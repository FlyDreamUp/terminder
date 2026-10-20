//! Byte accounting is independent of the host IPC implementation.
use crate::model::*;
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Default)]
pub struct GlobalBudget(AtomicUsize);
impl GlobalBudget {
    pub fn reserve(self: &Arc<Self>, bytes: usize) -> Option<Lease> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|next| *next <= GLOBAL_OUTPUT_BYTES)
            })
            .ok()?;
        Some(Lease {
            owner: self.clone(),
            bytes,
        })
    }
    pub fn used(&self) -> usize {
        self.0.load(Ordering::Acquire)
    }
}
pub struct Lease {
    owner: Arc<GlobalBudget>,
    bytes: usize,
}
impl Lease {
    pub fn shrink(&mut self, bytes: usize) {
        assert!(bytes <= self.bytes);
        self.owner.0.fetch_sub(self.bytes - bytes, Ordering::AcqRel);
        self.bytes = bytes;
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.owner.0.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
pub struct OutputBlock {
    pub bytes: Vec<u8>,
    pub lease: Lease,
}
struct InFlight {
    sequence: u64,
    total: u64,
    bytes: usize,
    _lease: Lease,
}
#[derive(Default)]
pub struct OutputFlow {
    sent_sequence: u64,
    sent_bytes: u64,
    ack_sequence: u64,
    ack_bytes: u64,
    in_flight: VecDeque<InFlight>,
    outstanding: usize,
}
impl OutputFlow {
    pub fn can_send(&self, size: usize) -> bool {
        size <= OUTPUT_CHUNK
            && self.outstanding + size <= OUTPUT_WINDOW_BYTES
            && self.in_flight.len() < OUTPUT_WINDOW_BLOCKS
    }
    // Reserve before calling the host. A failed send tears down the binding, never rolls bytes forward.
    pub fn submit(&mut self, block: OutputBlock) -> Result<Vec<u8>> {
        if !self.can_send(block.bytes.len()) {
            return Err(CoreError::new("output_full", "终端输出等待消费"));
        }
        self.sent_sequence = self
            .sent_sequence
            .checked_add(1)
            .ok_or_else(|| CoreError::new("counter_overflow", "输出计数耗尽"))?;
        self.sent_bytes = self
            .sent_bytes
            .checked_add(block.bytes.len() as u64)
            .ok_or_else(|| CoreError::new("counter_overflow", "输出计数耗尽"))?;
        self.outstanding += block.bytes.len();
        self.in_flight.push_back(InFlight {
            sequence: self.sent_sequence,
            total: self.sent_bytes,
            bytes: block.bytes.len(),
            _lease: block.lease,
        });
        Ok(block.bytes)
    }
    pub fn ack(&mut self, sequence: u64, bytes: u64) -> Result<()> {
        if sequence == self.ack_sequence && bytes == self.ack_bytes {
            return Ok(());
        }
        let valid = self
            .in_flight
            .iter()
            .any(|f| f.sequence == sequence && f.total == bytes);
        if !valid {
            return Err(CoreError::new(
                "invalid_ack",
                "输出确认不匹配已发送的连续前缀",
            ));
        }
        while self
            .in_flight
            .front()
            .is_some_and(|f| f.sequence <= sequence)
        {
            let frame = self.in_flight.pop_front().expect("front exists");
            self.outstanding -= frame.bytes;
        }
        self.ack_sequence = sequence;
        self.ack_bytes = bytes;
        Ok(())
    }
    pub fn end(&self) -> (u64, u64) {
        (self.sent_sequence, self.sent_bytes)
    }
    pub fn pending(&self) -> bool {
        !self.in_flight.is_empty()
    }
    pub fn abandon(&mut self) {
        self.in_flight.clear();
        self.outstanding = 0;
    }
}

#[derive(Default)]
pub struct InputOrder {
    last: u64,
    last_payload: Vec<u8>,
}
impl InputOrder {
    pub fn check(&self, sequence: u64, bytes: &[u8]) -> Result<bool> {
        if bytes.is_empty() || bytes.len() > INPUT_CHUNK {
            return Err(CoreError::new("input_size", "输入块须为 1–16384 字节"));
        }
        if sequence != 0 && sequence == self.last && bytes == self.last_payload {
            return Ok(false);
        }
        if self.last.checked_add(1) != Some(sequence) {
            return Err(CoreError::new(
                "input_order",
                "输入序列不连续或重复内容不匹配",
            ));
        }
        Ok(true)
    }
    pub fn accepted(&mut self, sequence: u64, bytes: &[u8]) {
        self.last = sequence;
        self.last_payload.clear();
        self.last_payload.extend_from_slice(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn block(budget: &Arc<GlobalBudget>, bytes: Vec<u8>) -> OutputBlock {
        OutputBlock {
            lease: budget.reserve(bytes.len()).unwrap(),
            bytes,
        }
    }
    #[test]
    fn acknowledgments_release_only_exact_prefixes() {
        let budget = Arc::new(GlobalBudget::default());
        let mut flow = OutputFlow::default();
        assert_eq!(
            flow.submit(block(&budget, vec![0xf0, 0x9f])).unwrap(),
            [0xf0, 0x9f]
        );
        flow.submit(block(&budget, vec![0x98, 0x80])).unwrap();
        assert_eq!(budget.used(), 4);
        assert!(flow.ack(2, 3).is_err());
        assert!(flow.ack(3, 4).is_err());
        assert_eq!(budget.used(), 4);
        flow.ack(1, 2).unwrap();
        flow.ack(1, 2).unwrap();
        assert_eq!(budget.used(), 2);
        flow.ack(2, 4).unwrap();
        assert_eq!(budget.used(), 0);
        assert!(flow.ack(1, 2).is_err());
    }
    #[test]
    fn stalled_consumer_has_a_hard_window() {
        let budget = Arc::new(GlobalBudget::default());
        let mut flow = OutputFlow::default();
        for _ in 0..8 {
            flow.submit(block(&budget, vec![1; OUTPUT_CHUNK])).unwrap();
        }
        assert!(!flow.can_send(1));
        assert_eq!(budget.used(), OUTPUT_WINDOW_BYTES);
        flow.ack(8, OUTPUT_WINDOW_BYTES as u64).unwrap();
        assert!(flow.can_send(OUTPUT_CHUNK));
    }
    #[test]
    fn global_budget_reclaims_every_lease() {
        let b = Arc::new(GlobalBudget::default());
        let lease = b.reserve(GLOBAL_OUTPUT_BYTES).unwrap();
        assert!(b.reserve(1).is_none());
        drop(lease);
        assert_eq!(b.used(), 0);
    }
    #[test]
    fn input_is_idempotent_only_for_matching_last_packet() {
        let mut order = InputOrder::default();
        assert!(order.check(1, b"a").unwrap());
        order.accepted(1, b"a");
        assert!(!order.check(1, b"a").unwrap());
        assert!(order.check(1, b"b").is_err());
        assert!(order.check(3, b"c").is_err());
        assert!(order.check(2, b"b").unwrap());
    }
}
