# RSA request signing for Bybit keys (watcher and OMS)

Status: ready-for-agent
Owner decision: junyoung 2026-11-25 00:55 KST (DM): "rsa키로 해". Bybit testnet AI sub-accounts only issue self-generated (RSA) keys; the system-generated (HMAC) option is not offered there.

## Problem
Both the watcher (crates/watcher) and the OMS Bybit venue (crates/oms-node/src/venue/bybit) sign every private request with HMAC-SHA256 only. The two testnet keys made for the owner's dogfood run (on the owner's testnet sub-account, IP-bound) are RSA keys. Neither binary can use them.

## Facts (measured on the Linux PC 2026-11-25 01:0x KST, Bybit testnet, with openssl + python, keys of the owner's testnet sub-account)
- F1 REST: `X-BAPI-SIGN = base64(RSA-SHA256 PKCS#1 v1.5 signature over timestamp + api_key + recv_window + payload)`, standard base64 with padding. Same prehash as HMAC; only the transform differs (Bybit v5 guide "Create A Request": "lowercase HEX string for HMAC_SHA256, or base64 for RSA_SHA256"). GET /v5/user/query-api signed this way: retCode 0.
- F2 private WebSocket `auth`: signature over `"GET/realtime" + expires`, RSA-SHA256, base64: `{"success":true}`. The same signature as lowercase hex: `{"success":false,"ret_msg":"Invalid sign"}`.
- F3 No extra header is needed (X-BAPI-SIGN-TYPE was not sent in F1).
- F4 Key files from `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048` are PKCS#8 PEM (`BEGIN PRIVATE KEY`).
- F5 `ring 0.17.14` and `base64 0.23.1` are already in Cargo.lock (via rustls/reqwest). ring signs RSA PKCS#1 v1.5 SHA-256 from PKCS#8 DER (`RsaKeyPair::from_pkcs8`, `RSA_PKCS1_SHA256`) and refuses moduli under 2048 bits.

## Decisions
D1 Credentials file (both binaries, same shape): TOML with `api_key` and exactly one of
   - `api_secret` (HMAC, unchanged behaviour), or
   - `rsa_private_key`: the PKCS#8 PEM text inline (TOML multi-line string).
   Both or neither: refused at start with one line that does not quote the file. Unknown keys still refused. The file rules (regular file, owner, 0600/0400, outside data_dir, no symlink) are unchanged and now also protect the RSA key. Inline (not a second path) so there is one secret file with one set of checks.
D2 PEM rules: only a PEM whose header line is `BEGIN PRIVATE KEY` between five dashes on each side (PKCS#8, unencrypted; the full header is not written here so that tools/ci/secret_scan.sh passes). A `BEGIN RSA PRIVATE KEY` (PKCS#1) or `BEGIN ENCRYPTED PRIVATE KEY` is refused with a line that names the fix (`openssl pkcs8 -topk8 -nocrypt -in old.pem -out new.pem`). Modulus under 2048 bits refused (ring). Parse once at load; a key that does not parse is a start refusal (exit 2 in the watcher like the other credential errors; the OMS's existing credential-error path).
D3 One signer type per crate: the credentials hold an enum (HMAC secret | parsed RSA key pair) and one `sign(prehash) -> String` that returns lowercase hex for HMAC and standard base64 for RSA. Every call site (watcher GET + cancel-all POST; OMS REST GET/POST and private-stream auth) goes through it. No call site keeps its own HMAC call.
D4 Watcher independence stays: the watcher gets its own implementation written from F1–F3 and the Bybit docs, not shared code with oms-* (tools/ci/watcher_independent.sh must pass).
D5 Dependencies: add `ring = "0.17"` and `base64 = "0.23"` to crates/watcher and crates/oms-node only if Cargo.lock gains no new package (`cargo build --locked` must pass). If it would add a package, stop and write BLOCKED.
D6 Debug/Display of credentials never prints the secret or the key (as today for the secret).
D7 RSA signing failure (ring returns Err) is not expected for a parsed key; treat it as a request that was not sent (the existing NotSent/refusal path of each crate), never send an unsigned or HMAC-signed request instead.

## Done criteria → tests
- T1 (each crate) HMAC signatures are byte-identical to before for the existing vectors (existing tests keep passing unchanged).
- T2 (each crate) RSA: for a fixed test key and fixed prehash, the signature equals a fixed expected base64 string computed outside the code with `openssl dgst -sha256 -sign key.pem | base64 -w0` (record the command in the test comment). PKCS#1 v1.5 is deterministic, so this is an exact cross-implementation check. Also verify it with ring's public key (`RSA_PKCS1_2048_8192_SHA256`).
- T3 (OMS) the private-stream auth message with an RSA key carries the base64 signature of `GET/realtime{expires}` (F2).
- T4 (each crate) credentials file refusals: both secrets; neither; PKCS#1 PEM; encrypted PEM; garbage PEM; 1024-bit key. Each error line does not contain any byte of the file's key material (assert the PEM body substring is absent).
- T5 (each crate) loopback fake (existing http-fake helpers): a GET and (watcher) the cancel-all POST / (OMS) an order POST sent with an RSA key carry `X-BAPI-SIGN` equal to the expected base64 for the exact query/body sent.
- T6 Debug of RSA credentials hides the key.
Test keys: generate them once with openssl (2048 and 1024 bits) and embed them in test code WITHOUT a literal PEM header in the source (build the header at run time, e.g. concat of "-----BEGIN " and "PRIVATE KEY-----", or embed the DER as hex/base64 body only), so secret scanners do not flag the repository; say in a comment that they are test-only keys. tools/ci/secret_scan.sh must pass without editing secret_scan_allow.txt.

## Documents and registry
- docs/operations/watcher.md "자격 파일": both forms, the openssl commands to make a key pair and how to register the public key (Bybit: API management → Create New Key → Self-generated API Keys), the refusal lines. The same for the OMS credentials doc (find where the OMS credentials file is documented; runbook or docs/operations).
- docs/contracts: the gateway signature row and C-WATCH part 3/4 (credentials) get the RSA form. Keep edits small (another change may restructure contracts).
- New ADR (next free number) "Bybit RSA 서명 키": context (AI sub-account, F1–F3), decision D1–D7, consequence. Status line: 채택.
- registry/claims.toml: claims for T2–T5 naming the real tests. Regenerate docs/generated.
- xtask mutants: at least: RSA signature hex instead of base64 (OMS ws auth and REST), RSA path falling back to HMAC, both-secrets accepted.

## Not in scope
Key rotation, Ed25519, HL signing, any real network call in tests.
