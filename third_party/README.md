# Upstream notice supplements

Some locked packages omit their license file from the registry archive. `notices/sources.json` records the package/version, original source and SHA-256 of each unmodified supplement. Rust revision IDs come from each package's `.cargo_vcs_info.json`; npm revision IDs come from the registry's published version metadata. The selectors package declares MPL-2.0 and references Mozilla's canonical version 2.0 license.

`is-reference` and `locate-character` publish only an MIT declaration in their README, including at the recorded upstream revision. Those exact README notices are retained; no copyright holder/year has been invented.

`scripts/release-notices.mjs` checks these hashes and includes notices plus exact source-archive locations in the installer's dependency inventory. New dependency versions that lack bundled notices require a new reviewed supplement. The repository `.gitattributes` disables Git text conversion for `third_party/notices/`, preserving the original bytes even when Windows checkouts use `core.autocrlf=true`. Keep the recorded upstream hashes unchanged; replacing a supplement requires reviewing its source and new hash.

The npm inventory excludes development-only and extraneous lockfile entries. An extraneous entry can remain in a local `node_modules` but is omitted by a clean `npm ci`; it is not a release dependency. Missing manifests for included dependencies still fail the build. Run `npm --prefix apps/desktop run test:notices` to check both installation states and these failure cases.
