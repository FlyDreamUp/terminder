use keyboard_types::{Key, Modifiers};

pub fn encode_key(key: &Key, modifiers: Modifiers) -> Option<Vec<u8>> {
    let mut bytes = match key {
        Key::Enter => vec![b'\r'],
        Key::Backspace => vec![0x7f],
        Key::Tab if modifiers.shift() => b"\x1b[Z".to_vec(),
        Key::Tab => vec![b'\t'],
        Key::Escape => vec![0x1b],
        Key::ArrowUp => b"\x1b[A".to_vec(),
        Key::ArrowDown => b"\x1b[B".to_vec(),
        Key::ArrowRight => b"\x1b[C".to_vec(),
        Key::ArrowLeft => b"\x1b[D".to_vec(),
        Key::Home => b"\x1b[H".to_vec(),
        Key::End => b"\x1b[F".to_vec(),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        Key::Delete => b"\x1b[3~".to_vec(),
        Key::Character(ch) => encode_character(ch, modifiers)?,
        _ => return None,
    };

    if modifiers.alt() && !matches!(key, Key::Character(_)) {
        let mut prefixed = vec![0x1b];
        prefixed.append(&mut bytes);
        return Some(prefixed);
    }

    Some(bytes)
}

fn encode_character(text: &str, modifiers: Modifiers) -> Option<Vec<u8>> {
    if text.is_empty() {
        return None;
    }

    let mut chars = text.chars();
    let first = chars.next()?;

    if modifiers.ctrl() {
        let control = match first.to_ascii_lowercase() {
            'a'..='z' => (first.to_ascii_lowercase() as u8) - b'a' + 1,
            ' ' => 0,
            '[' => 27,
            '\\' => 28,
            ']' => 29,
            '^' => 30,
            '_' => 31,
            _ => return None,
        };
        return Some(vec![control]);
    }

    let mut payload = text.as_bytes().to_vec();
    if modifiers.alt() {
        let mut prefixed = vec![0x1b];
        prefixed.append(&mut payload);
        return Some(prefixed);
    }

    Some(payload)
}

#[cfg(test)]
mod tests {
    use keyboard_types::{Key, Modifiers};

    use super::encode_key;

    #[test]
    fn encode_ctrl_c() {
        let bytes = encode_key(&Key::Character("c".to_string()), Modifiers::CONTROL);
        assert_eq!(bytes, Some(vec![3]));
    }

    #[test]
    fn encode_arrow_up() {
        let bytes = encode_key(&Key::ArrowUp, Modifiers::empty());
        assert_eq!(bytes, Some(b"\x1b[A".to_vec()));
    }
}
