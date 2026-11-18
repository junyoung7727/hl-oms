# 02: zstd frames: writer with block flush, frame scanner, stream reader

**What to build:** A frame writer (`level`, `pledged_size`, `flush()` = `ZSTD_e_flush`, `finish()`), a scanner over concatenated frames that reports each frame's end and, for an unfinished last frame, the offset after its last complete block (RFC 8878 frame header, block headers, optional checksum), and a reader that decodes concatenated frames into one QRAW1 stream. A codec factory trait counts live compression contexts and their estimated sizes. Spec: "Solution" 2.

**Blocked by:** 01

**Status:** done

- [x] Test `concatenated_frames_decode_to_one_stream`: a day file of three level-9 frames (first with magic and header) decodes to one stream whose `rec_no` runs 0..n without a break.
- [x] Test `scanner_finds_frame_ends_and_the_last_complete_block`: for a finished stream, every frame end; for a stream cut at every byte offset after the header, the last complete block offset is ≤ the cut and decoding up to it gives a record prefix of the original.
- [x] Test `pledged_size_and_live_contexts_are_counted`: the factory records the pledged size given to each frame and the live contexts; a small pledged size gives a smaller measured level-9 context.
- [x] Test `a_record_across_a_frame_boundary_is_refused` (review F14).

## Comments

- `ZSTD_estimateCStreamSize_usingCParams` needs `unsafe` FFI, which the workspace forbids (`unsafe_code = "forbid"`). `context_bytes` measures a probe context instead (`ZSTD_sizeof_CCtx` after init, pledged size rounded up to a power of two, cached).
- Two mutations survived the first test version (a mid-block cut reported as complete; no block flush at all). The scanner test now pins every expected block end and checks that each flush grows the file.
