# 02: zstd frames: writer with block flush, frame scanner, stream reader

**What to build:** A frame writer (`level`, `pledged_size`, `flush()` = `ZSTD_e_flush`, `finish()`), a scanner over concatenated frames that reports each frame's end and, for an unfinished last frame, the offset after its last complete block (RFC 8878 frame header, block headers, optional checksum), and a reader that decodes concatenated frames into one QRAW1 stream. A codec factory trait counts live compression contexts and their estimated sizes. Spec: "Solution" 2.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Test `concatenated_frames_decode_to_one_stream`: a day file of three level-9 frames (first with magic and header) decodes to one stream whose `rec_no` runs 0..n without a break.
- [ ] Test `scanner_finds_frame_ends_and_the_last_complete_block`: for a finished stream, every frame end; for a stream cut at every byte offset after the header, the last complete block offset is ≤ the cut and decoding up to it gives a record prefix of the original.
- [ ] Test `pledged_size_is_the_counted_bytes`: the factory records the pledged size given to each frame.
