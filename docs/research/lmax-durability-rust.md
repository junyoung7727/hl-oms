# LMAX 원칙 · 내구성 프리미티브 · Rust 생태계 — 1차 자료 조사

- 작성일(접근일): 2026-10-02
- 대상 환경: 단일 Linux 호스트(kernel 7.0), ext4, 소비자용 NVMe SSD(Samsung 980, PLP 없음 — 소유자 전제), Rust 1.96 stable
- 표기 규칙
  - **원문**: 출처에서 그대로 옮긴 영어 인용(30단어 이하). 인용 뒤 괄호는 절/섹션 이름.
  - **제안**: 조사자의 설계 제안. 출처의 진술이 아니다.
  - **추론**: 출처 문장들을 조합한 조사자의 해석. 출처가 직접 말한 것이 아니다.
  - **확인 불가**: 접근한 범위에서 해당 진술을 찾지 못함.
- 모든 수치·버전은 이번 조사에서 실제로 가져온 페이지/API 응답에서 읽은 것이다. 기억에 의존한 값은 쓰지 않았다.

---

## 0. 접근 기록

### 0.1 문서·논문

| # | 출처 | 버전/날짜 (페이지에 표기된 것) | URL | 접근 범위 | 문제 |
|---|---|---|---|---|---|
| A1 | Martin Fowler, "The LMAX Architecture" | 12 July 2011 (Significant Revisions: first publication) | https://martinfowler.com/articles/lmax.html | 전문(본문+각주 1–17) | 그림(Figure 1–4)은 텍스트 추출로 읽지 못함 |
| A2 | LMAX Disruptor 기술 논문 (HTML판) | "version 4.0.0-SNAPSHOT, May 2011", Last updated 2025-04-02 | https://lmax-exchange.github.io/disruptor/disruptor.html | 전문 | HTML판에는 2011 원본에 없는 현대 하드웨어(AMD EPYC) 처리량 표(Table 3)가 추가돼 있음 |
| A3 | 동 논문 원본 PDF | Disruptor-1.0.pdf, 11쪽 | https://lmax-exchange.github.io/disruptor/files/Disruptor-1.0.pdf | 수치 대조용(표, 지연 문단) | PDF에서는 지연 표가 "Table 3" |
| A4 | LMAX Disruptor User Guide | 4.0.0-SNAPSHOT, Last updated 2025-04-02 | https://lmax-exchange.github.io/disruptor/user-guide/index.html | 개념·Wait Strategy·Batch Rewind 절 | GitHub wiki 자체는 별도로 열지 않음(가이드가 대체) |
| B1 | fsync(2)/fdatasync(2) | Linux man-pages 6.19 (2026-02-08), HTML 2026-09-09 | https://man7.org/linux/man-pages/man2/fsync.2.html | 전문 | fdatasync.2.html은 같은 페이지 |
| B2 | open(2) | man-pages 6.19 | https://man7.org/linux/man-pages/man2/open.2.html | O_APPEND/O_DIRECT/O_DSYNC/O_SYNC, "Synchronized I/O", O_DIRECT NOTES | — |
| B3 | write(2) | man-pages 6.19 | https://man7.org/linux/man-pages/man2/write.2.html | DESCRIPTION, RETURN VALUE, ERRORS(EIO), NOTES, BUGS | — |
| B4 | rename(2) | man-pages 6.19 | https://man7.org/linux/man-pages/man2/rename.2.html | DESCRIPTION | 크래시 내구성에 대한 문장은 없음 |
| B5 | fallocate(2) | man-pages 6.19 | https://man7.org/linux/man-pages/man2/fallocate.2.html | "Allocating disk space" | — |
| B6 | sync_file_range(2) | man-pages 6.19 | https://man7.org/linux/man-pages/man2/sync_file_range.2.html | DESCRIPTION, Warning | — |
| B7 | flock(2) | man-pages 6.19 | https://man7.org/linux/man-pages/man2/flock.2.html | DESCRIPTION | — |
| B8 | Rust std `File` (`sync_all`, `sync_data`, `set_len`, `lock*`) | rustdoc 1.96.0 (ac68faa20 2026-05-25) | https://doc.rust-lang.org/1.96.0/std/fs/struct.File.html | 해당 메서드 | 첫 시도 connection reset, 재시도 성공. `/std/`(현재 stable)는 1.99.0 (2026-09-28)이며 sync_all/sync_data 문구 동일 |
| B9 | Rust std `OpenOptions::append` | rustdoc 1.96.0 | https://doc.rust-lang.org/1.96.0/std/fs/struct.OpenOptions.html | `append` | 첫 시도 reset, 재시도 성공 |
| B10 | Rust std `fs::rename` | rustdoc 1.96.0 | https://doc.rust-lang.org/1.96.0/std/fs/fn.rename.html | 전문 | — |
| B11 | Rust std 소스 (unix fs) | tag 1.96.0 | https://raw.githubusercontent.com/rust-lang/rust/1.96.0/library/std/src/sys/fs/unix.rs , `.../sys/pal/unix/mod.rs` | `fsync`, `datasync`, `truncate`, `rename`, `cvt_r` | 문서가 아니라 구현(보장 아님) |
| B12 | PostgreSQL wiki "Fsync Errors" | last edited 5 July 2023 (oldid=38031) | https://wiki.postgresql.org/wiki/Fsync_Errors | 전문 | — |
| B13 | Rebello et al., "Can Applications Recover from fsync Failures?" (USENIX ATC'20) | July 15–17, 2020 | https://www.usenix.org/conference/atc20/presentation/rebello , PDF https://www.usenix.org/system/files/atc20-rebello.pdf | Abstract, §1–3.3.1(ext4), Table 1, §5 | 발표 페이지 첫 시도 reset, 재시도 성공. §4(애플리케이션별)는 일부만 |
| B14 | Pillai et al., "All File Systems Are Not Created Equal" (OSDI'14) | 2014 | https://www.usenix.org/conference/osdi14/technical-sessions/presentation/pillai , PDF https://www.usenix.org/system/files/conference/osdi14/osdi14-paper-pillai.pdf | Abstract/§1, §2(Table 1), §4.3–4.4 | Table 1은 그림이라 300dpi 렌더 이미지를 눈으로 읽음(열 판독은 조사자 책임). 시험 커널 버전은 확인 불가 |
| B15 | SQLite "Atomic Commit In SQLite" | last updated 2026-04-21 | https://www.sqlite.org/atomiccommit.html | §2 가정, §3.7, §5, §6.2, §9.2–9.4 | — |
| B16 | SQLite "How To Corrupt An SQLite Database File" | last updated 2026-04-13 | https://www.sqlite.org/howtocorrupt.html | §3, §4.1 | — |
| B17 | SQLite "Write-Ahead Logging" | last updated 2026-08-25 | https://www.sqlite.org/wal.html | sync 관련 문단 | — |
| B18 | SQLite "Database File Format" (WAL 프레임 유효성) | last updated 2025-12-25 | https://www.sqlite.org/fileformat2.html | §4 WAL | 과업 목록에는 없으나 체크섬 규칙 확인용으로 추가 |
| B19 | ext4 General Information (kernel doc) | docs.kernel.org 표기 "7.3.0-rc5" | https://docs.kernel.org/admin-guide/ext4.html | mount options, Data Mode | 호스트 커널(7.0)과 문서 버전(7.3-rc5)이 다름 |
| B20 | "Explicit volatile write back cache control" (kernel doc) | 7.3.0-rc5 | https://docs.kernel.org/block/writeback_cache_control.html | 전문 | 과업 목록 외 추가 |
| B21 | LWN "PostgreSQL's fsync() surprise" (2차) | April 18, 2018 | https://lwn.net/Articles/752063/ | 앞부분 | 2차 자료 |
| B22 | LWN "Improved block-layer error handling" (2차) | June 2, 2017 | https://lwn.net/Articles/724307/ | errseq_t 문단 | 2차 자료 |
| B23 | LWN "Ensuring data reaches disk" (2차) | September 7, 2011 | https://lwn.net/Articles/457667/ | rename 절차, write-back cache 절 | 2차 자료, 오래됨 |
| H1 | 호스트 관찰(읽기 전용) | 2026-10-02 | `/sys/block/nvme*/queue/*`, `/proc/fs/ext4/nvme1n1p2/options`, `uname -r`, `rustc --version` | — | 출처가 아니라 현장 값 |

### 0.2 crate 메타데이터

| # | 출처 | 버전/날짜 | URL | 접근 범위 | 문제 |
|---|---|---|---|---|---|
| C1 | crates.io API | 2026-10-02 응답 | https://crates.io/api/v1/crates/&lt;name&gt; | 약 95개 crate: `max_stable_version`, 해당 버전 `created_at`, `license`, `rust_version`, `yanked` | `turmoil-fs`는 "does not exist" 응답 |
| C2 | RustSec advisory-db | commit `117edb3bed98` (2026-10-02T10:58:33+02:00) | https://github.com/rustsec/advisory-db | `crates/<name>/` 디렉터리 유무와 각 권고의 TOML front matter | "없음" = 해당 디렉터리가 없다는 뜻 |
| C3 | docs.rs | 각 crate의 위 버전 | https://docs.rs/&lt;crate&gt;/&lt;version&gt;/… | crate 최상위 문서와 표에 적은 항목 | 전 항목을 정독한 것은 아님 |
| C4 | GitHub (README, 저장소 메타) | 2026-10-02 | 각 저장소 | disruptor-rs, miri, shuttle, turmoil, madsim, cargo-mutants README; `archived`/`pushed_at` | hyperliquid-rust-sdk는 master `aac75585daf1` (2025-10-21) |
| C5 | bincode 3.0.0 crate 파일 | 2025-12-16 | https://static.crates.io/crates/bincode/bincode-3.0.0.crate | README.md, src/lib.rs | — |

---

## Part 1 — LMAX 1차 자료

성능 수치는 전부 **LMAX가 자기 조건(Java, 2011년 하드웨어)에서 측정한 값**이다. 이 OMS의 목표치나 기대치로 쓸 수 없다.

### 1.1 Business Logic Processor (BLP)

**무엇인가**

- 원문: "The Business Logic Processor runs entirely in-memory using event sourcing." (A1, 서두 요약)
- 원문: "it does this as a single-threaded java program which reacts to method calls and produces output events" (A1, Overall Structure)
- 원문: "It operates entirely in-memory, there is no database or other persistent store." (A1, Keeping it all in memory)
- 원문: "the current state of the Business Logic Processor is entirely derivable by processing the input events" (A1, Keeping it all in memory)
- "Business Logic Processor"는 Fowler가 붙인 이름이다. 원문: "The team does not use the name Business Logic Processor, in fact they have no name for that component" (A1, 각주 5)

**결정성**

- 원문: "The business logic is deterministic and very fast, so there's no gain from storing the results." (A1, 각주 12)
- 원문: "The leader node provides a deterministic sequence for the rest of the processing." (A1, Input and Output Disruptors) — 여기서 결정성은 **입력 순서**를 리더가 정한다는 뜻이다.
- 결정성을 *어떻게* 유지하는지(난수, 반복 순서, 부동소수 등)에 대한 규칙은 A1/A2/A4에 없다. **확인 불가**.

**시간 처리**

- A1, A2, A4 어디에도 시계/타임스탬프를 로직에 어떻게 주입하는지에 대한 문장이 없다(`clock`, `timestamp`, `current time` 검색 결과 무관한 문맥뿐). **확인 불가**. "시간을 입력 이벤트로 넣는다"는 흔한 설명은 이 세 문서의 진술이 아니다.

**외부 서비스 호출**

- 원문: "As a result you can't make calls to external services within the business logic." (A1, Programming Model)
- 원문: "Instead you need to finish that interaction with an output event, and wait for another input event to pick it back up again." (A1, Programming Model)
- 예시(신용카드 검증)에서 BLP는 요청 이벤트를 내보내고, 원문: "carry on processing events for other customers until it received a credit-card-validated event in its input event stream" (A1, Programming Model)

**오류 처리 / 롤백**

- 원문: "However there's no automated rollback facility." (A1, Programming Model)
- 원문: "ensuring the input events are fully valid before doing any mutation of the in-memory persistent state" (A1, Programming Model)
- 원문: "They have found that testing is a key tool in flushing out these kinds of problems before going into production." (A1, Programming Model)

### 1.2 입력 디스럽터: journaler · replicator · unmarshaller

**세 작업과 순서**

- 원문: "Input messages need to be taken off a network gateway and unmarshaled, replicated and journaled." (A1, Overall Structure)
- 원문: "all of them need to be done before the Business Logic Processor works on a message, but they can done in any order" (A1, Input and Output Disruptors; "can done"은 원문 그대로)
- 원문: "it is necessary to prevent the business logic consumer from making progress until the journalling and replication consumers have completed their tasks" (A4, Consumer Dependency Graph)
- 원문: "The ApplicationConsumer depends on the JournalConsumer and ReplicationConsumer." (A4, Consumer Dependency Graph)

→ **질문에 대한 답**: 그렇다. 두 문서 모두 비즈니스 로직은 저널링 **그리고** 복제가 끝난 메시지만 처리한다고 말한다. journaler·replicator·unmarshaller 셋끼리는 병렬이다.

단, "저널링이 끝났다"가 `fsync` 완료를 뜻하는지는 어디에도 적혀 있지 않다. 원문은 "They stream the events onto the disk."(A1)까지만 말한다. **확인 불가**.

**journaler**

- 원문: "LMAX does not use a database for this, just the file system. They stream the events onto the disk." (A1, Input and Output Disruptors)
- 원문: "The replicator and journaler involve IO and therefore are relatively slow." (A1, 같은 절)
- 확장 여지: 원문: "If you ran two journalers then one would take the even slots and the other journaler would take the odd slots." (A1)

**부하 시 배칭**

- A1에 "journaler가 배치로 쓴다"는 직접 문장은 없다. 있는 것은 소비자 일반의 따라잡기 동작이다. 원문: "it can read data from slots 16-30 in one batch to catch up" (A1, unmarshaler 예시)
- 원문: "it can process up to that sequence without getting involved in the concurrency mechanisms" (A2, 3.4 Batching Effect)
- 원문: "This type of batching increases throughput while reducing and smoothing latency at the same time." (A2, 3.4)
- 배치 끝 콜백의 용도: 원문 코드 주석 "useful for flushing results to an IO device if necessary." (A2, 3.7 Code Example, `onEndOfBatch`)
- 조기 해제 예제 주석: "If this is doing I/O, it may be after flushing/syncing to disk" (A4, Dealing With Large Batches)
- **추론**: 이 조합은 "밀린 만큼 한 번에 쓰고 배치 끝에서 flush" 형태의 그룹 커밋을 가리키지만, LMAX journaler가 실제로 그렇게 했다는 진술은 접근 범위에 없다.

**replicator**

- 원문: "Only the leader node listens directly to input events and runs a replicator. The replicator broadcasts the input events to the follower nodes." (A1)
- 원문: "Each node has its own input disruptor and thus has its own journal and does its own unmarshaling." (A1)
- 복제가 필요한 이유: 원문: "IP messages can arrive in a different order on different nodes" (A1)

**unmarshaller**

- 원문: "each writable field can only have one parallel consumer that's allowed to write to it" (A1) — 링 버퍼 엔트리에 쓰되 필드별 단일 작성자 규칙을 지킨다.

**링 버퍼 크기**

- 원문: "20 million slots for input buffer and 4 million slots for each of the output buffers" (A1)

### 1.3 출력 측

- 원문: "Output disruptors are similar but they only have two sequential consumers for marshaling and output." (A1)
- 원문: "Output events are organized into several topics" / "Each topic has its own disruptor." (A1)

**재생 시 출력이 어떻게 되는가 — 원문이 실제로 말하는 것**

1. 복제 노드의 출력: 원문: "Each input event is processed by multiple processors, but all but one processor has its output ignored." (A1, Keeping it all in memory)
2. 출력 저널링: 원문: "It would be possible to journal the output events too." 이어서 "This would have the advantage of not needing to recalculate them should they need to be replayed for downstream services." (A1, 각주 12)
3. 결론: 원문: "In practice, however, this isn't worthwhile." (A1, 각주 12)

→ 원문이 말하는 것은 (i) 리더가 아닌 노드의 출력은 무시된다, (ii) 출력은 저널하지 않고 필요하면 재계산한다, 두 가지뿐이다. **재시작/재생 중에 이미 나갔던 외부 메시지를 다시 보내는지 억제하는지, 중복 제거를 어떻게 하는지에 대한 문장은 A1·A2·A4에 없다. 확인 불가.**

OMS에는 이 공백이 치명적이다. LMAX의 출력은 "하류에 재생해 줄 수 있는 계산 결과"로 묘사되지만, OMS의 출력(거래소 주문 전송)은 재계산해서 다시 보내면 중복 주문이 된다. 이 부분은 원문에서 답을 가져올 수 없고 따로 설계해야 한다(§1.8 제안 참조).

### 1.4 스냅샷 · 재시작 · 이중화

- 스냅샷 주기: 원문: "They take a snapshot every night during periods of low activity." (A1)
- 재시작 시간: 원문: "a full restart - including restarting the JVM, loading a recent snapshot, and replaying a days worth of journals - takes less than a minute" (A1)
- 스냅샷만으로는 부족한 이유: 원문: "but not quickly enough should a Business Logic Processor crash at 2pm" (A1)
- 노드 구성: 원문: "Currently LMAX runs two Business Logic Processors in its main data center and a third at a disaster recovery site. All three process input events." (A1, 각주 6)
- 전환 시간: 원문: "By event sourcing into replicas they can switch between processors in a matter of micro-seconds." (A1)
- 장애 전환: 원문: "Should the leader node go down, it's lack of heartbeat will be noticed, another node becomes leader, starts processing input events, and starts its replicator." (A1)
- 야간 재시작: 원문: "they also restart the Business Logic Processors every night" (A1), "Like the rest of the system, the disruptors are bounced overnight." (A1). 이유는 메모리를 비워 거래 중 GC 가능성을 줄이는 것(Java 고유).
- 진단용 재생: 원문: "the team copies the sequence of events to their development environment and replays them there" (A1)

스냅샷 파일 형식, 스냅샷과 저널 위치의 대응 기록 방식, 스냅샷 검증 방법은 **확인 불가**.

### 1.5 성능 주장과 조건

**Fowler 기사**

- 원문: "A thread that will process 6 million orders per second using commodity hardware." (A1)
- 조건: 원문: "The 6 million TPS benchmark was measured on a 3Ghz dual-socket quad-core Nehalem based Dell server with 32GB RAM." (A1, 각주 4)
- 이 수치는 **BLP 단일 스레드의 처리량**이다. 저널 fsync나 네트워크를 포함한 종단 수치라는 진술은 없다.
- 단계별: 순차·인메모리만으로 "10K TPS", 잘 쪼갠 코드로 "100K TPS range", 그 이상은 캐시 친화 자료구조와 성능 시험(A1, Tuning performance).
- 기사에 종단 지연 수치는 없다. **확인 불가**.
- 원문: "as long as you need less than a few million TPS, you'll have enough performance headroom" (A1, Should you use this architecture?)

**Disruptor 논문 — 처리량 (A2 Table 2, A3와 대조)**

설정: Java 1.6.0_25 64-bit Sun JVM. 원문: "taking the best of 3 runs when processing 500 million messages" / "Results can vary substantially across different JVM executions and the figures below are not the highest we have observed." (A2, §4)

| 구성 | Nehalem 2.8GHz, Win7 — ABQ | — Disruptor | Sandy Bridge 2.2GHz, Linux 2.6.38 — ABQ | — Disruptor |
|---|---|---|---|---|
| Unicast 1P–1C | 5,339,256 | 25,998,336 | 4,057,453 | 22,381,378 |
| Pipeline 1P–3C | 2,128,918 | 16,806,157 | 2,006,903 | 15,857,913 |
| Sequencer 3P–1C | 5,539,531 | 13,403,268 | 2,056,118 | 14,540,519 |
| Multicast 1P–3C | 1,077,384 | 9,377,871 | 260,733 | 10,860,121 |
| Diamond 1P–3C | 2,113,941 | 16,143,613 | 2,082,725 | 15,295,197 |

(단위 ops/sec. ABQ = `java.util.concurrent.ArrayBlockingQueue`. HTML판 A2에는 AMD EPYC 9374F / Linux 5.4.277 / OpenJDK 11.0.24 표가 추가돼 있으나 2011 원본 PDF에는 없다.)

**지연 (A2 Table 4 = A3 Table 3)**

설정: 3단 파이프라인, 원문: "waiting 1 microsecond after injecting an event before injecting the next and repeating 50 million times", 2.2GHz Core i7-2720QM, Java 1.6.0_25, Ubuntu 11.04. 원문: "No CPU binding has been employed for this test." (A2, §5)

| | ArrayBlockingQueue (ns) | Disruptor (ns) |
|---|---|---|
| Min | 145 | 29 |
| Mean | 32,757 | 52 |
| 99% 이하 | 2,097,152 | 128 |
| 99.99% 이하 | 4,194,304 | 8,192 |
| Max | 5,069,086 | 175,567 |

- 원문: "Mean latency per hop for the Disruptor comes out at 52 nanoseconds compared to 32,757 nanoseconds for ArrayBlockingQueue." (A2, §5)
- 이 수치는 **스레드 간 전달 지연**이며 디스크·네트워크 I/O를 포함하지 않는다.

**경합 비용 실험 (A2 Table 1)** — 64비트 카운터를 5억 번 증가, 2.4GHz Intel Westmere EP, Java:

| 방법 | 시간(ms) |
|---|---|
| Single thread | 300 |
| Single thread with lock | 10,000 |
| Two threads with lock | 224,000 |
| Single thread with CAS | 5,700 |
| Two threads with CAS | 30,000 |
| Single thread with volatile write | 4,700 |

원문: "The language is unimportant to this experiment and results will be similar across all languages with the same basic primitives." (A2, 2.1) — LMAX의 주장이며, Rust에서 재측정한 것은 아니다.

### 1.6 Mechanical sympathy

| 주제 | 원문 |
|---|---|
| 락 비용 | "Locks are incredibly expensive because they require arbitration when contended." (A2, 2.1) |
| CAS 비용 | "This CAS approach is significantly more efficient than locks because it does not require a context switch to the kernel for arbitration." / "However CAS operations are not free of cost." (A2, 2.2) |
| 이상적 형태 | "The ideal algorithm would be one with only a single thread owning all writes to a single resource with other threads reading the results." (A2, 2.2) |
| false sharing | "they present the same problems of write contention as if they were a single variable" (A2, 2.4) |
| 캐시 라인 패딩 | "the Disruptor's sequence counter are padded to ensure they appear in separate cache lines" (A1, 각주 17) |
| 단일 작성자 | "you need a design that has only one core writing to any memory location" (A1, Queues and their lack of mechanical sympathy) |
| 큐의 문제 | "Queue implementations tend to have write contention on the head, tail, and size variables." (A2, 2.5) |
| 큐는 늘 가득 차거나 비어 있음 | "When in use, queues are typically always close to full or close to empty due to the differences in pace between consumers and producers." (A2, 2.5) |
| 무제한 큐 | "it can grow unchecked until it reaches the point of catastrophic failure by exhausting memory" (A2, 2.5) |
| 액터 프로토타입 결과 | "the processors spent more time managing queues than doing the real logic of the application" (A1) |
| 사전 할당 | "All memory for the ring buffer is pre-allocated on start up." (A2, 3.1) |
| 2의 거듭제곱 크기 | "This cost can be greatly reduced by making the ring size a power of 2." (A2, 3.2) |
| 단일 생산자 | "In the case of having only one producer, and regardless of the complexity of the consumer graph, no locks or CAS operations are required." (A2, 3.3) |
| 배칭 효과 | "this effect results in a close to constant time for latency regardless of load, up until the memory sub-system is saturated" (A2, 3.4) |
| 시험이 근거 | "in the end it's the testing gives you the convincing evidence" (A1) |
| null 컴포넌트 시험 | "writing tests against null components to ensure the performance test is fast enough" (A1) |

**Wait strategy와 CPU/지연 절충**

| 전략 | 원문 (A4, Alternative Wait Strategies / Optionally Lock-free) | 절충 |
|---|---|---|
| Blocking (기본) | "The default WaitStrategy used by the Disruptor is the BlockingWaitStrategy." / "The BlockingWaitStrategy is the slowest of the available wait strategies, but is the most conservative with the respect to CPU usage" | CPU 최소, 지연·지터 최대. 락+조건변수 사용 |
| Sleeping | "On a typical Linux system this will pause the thread for around 60µs." / "A common use case is for asynchronous logging." | 생산자 부담 최소, 평균 지연 높음 |
| Yielding | "This is the recommended wait strategy when you need very high performance, and the number of EventHandler threads is lower than the total number of logical cores" | CPU를 태워 지연을 낮춤 |
| BusySpin | "This wait strategy should only be used if the number of EventHandler threads is lower than the number of physical cores on the box" | 최고 성능, 배치 환경 제약 최대 |

- 원문: "Many low-latency systems will use a busy-wait to avoid the jitter that can be incurred by using a condition" (A4, Optionally Lock-free)
- 논문의 일반 서술: 원문: "If CPU resource is precious they can wait on a condition variable within a lock that gets signalled by the producers." / "This could be done with or without a thread yield by trading CPU resource against latency." (A2, 3.2)

### 1.7 자료구조 · 프리미티브 · GC — Java 고유 vs 언어 중립

**Java/JVM 고유로 원문이 명시한 것**

- 원문: "custom implementations of the java collections that were designed to be cache-friendly and careful with garbage" (A1, Tuning performance)
- 원문: "using primitive java longs as hashmap keys with a specially written array backed Map implementation (LongToObjectHashMap)" (A1)
- 원문: "The problematic objects are those that will get promoted to an older generation, but will eventually die." (A1, 각주 8)
- 원문: "The limitations of the Java language mean that entries are associated with the ring-buffer as pointers to objects." (A2, 3.1)
- 원문: "In Java there is a further problem with the use of queues, as they are significant sources of garbage." (A2, 2.5)
- 원문: "This pre-allocation of entries eliminates issues in languages that support garbage collection" (A2, 3.1)
- 야간 재시작의 목적(메모리 비우기)과 "Hotspot"이 작은 메서드를 더 잘 최적화한다는 서술(A1)도 JVM 고유다.

**언어 중립으로 읽히는 것**(원문이 하드웨어 수준에서 서술)

- 캐시 라인/false sharing, 단일 작성자, 락·CAS 경합 비용, 예측 가능한 메모리 stride(원문: "data structures like linked lists and trees tend to have nodes that are more widely distributed in memory with no predictable stride of access", A2 2.4), 유계 큐의 배압, 배칭 효과, 2의 거듭제곱 링.
- 원문: "In general they've found that choice of data structures often makes a big difference" (A1)

Rust에는 GC가 없으므로 "GC 회피" 항목은 그대로 옮겨지지 않는다. 대응 관심사는 핫 패스의 할당/해제 지터일 텐데, 이것은 **추론**이며 원문의 진술이 아니다.

### 1.8 표: LMAX 원칙 → 원문 근거 → Rust 단일 노드 OMS 대응 후보 → 단순화 시 바뀌는 보장

"대응 후보"와 "바뀌는 보장" 열은 전부 **제안/추론**이다.

| LMAX 원칙 | 원문 근거 | Rust 단일 노드 OMS 대응 후보 (제안) | 단순화 시 바뀌는 보장 (추론) |
|---|---|---|---|
| 단일 스레드 비즈니스 로직 | "single-threaded java program which reacts to method calls and produces output events" (A1) | 전용 OS 스레드 하나가 `&mut State`를 소유. 입력은 유계 채널 하나로만 들어옴. 코어 안에서는 async/락 없음 | 없음(그대로 적용 가능). tokio 태스크로 돌리면 스케줄러가 끼어들어 "한 코어, 따뜻한 캐시" 전제가 약해짐 |
| 인메모리 + 입력 이벤트 소싱 | "entirely derivable by processing the input events" (A1) | append-only 입력 저널(길이+체크섬+버전 프레임). 상태는 저널의 순수 함수 | 저널 내구성은 Part 2의 가정에 묶임. LMAX는 다중 노드가 이를 보완했으나 단일 노드에는 없음 |
| 저널·복제 완료 후에만 로직 실행 | "prevent the business logic consumer from making progress until the journalling and replication consumers have completed their tasks" (A4) | (가) LMAX식: 코어를 `fdatasync` 완료 시퀀스 뒤에 게이트. (나) 변형: 코어는 먼저 처리하되 **외부 전송(출력 방출)만** 저널 내구 시퀀스 뒤에 게이트 | (가)는 원문과 같은 순서, 지연에 fsync 포함. (나)는 메모리 상태가 디스크보다 앞설 수 있음 — 크래시 시 그 차이는 사라지지만 외부에는 나간 적이 없으므로 관측 불가. (나)는 원문에 없는 설계이므로 별도 검증 필요 |
| replicator / 다중 노드 | "All three process input events." (A1 각주 6) | 없음(단일 노드) | **핫 스탠바이 없음**: 장애 시 "micro-seconds" 전환 대신 재시작+재생 시간만큼 중단. 호스트/디스크 소실 시 저널도 소실. 리더가 주던 "deterministic sequence"는 단일 수신 지점이 대신함 |
| 외부 호출 금지, 출력 이벤트 → 입력 이벤트로 재진입 | "finish that interaction with an output event, and wait for another input event to pick it back up again" (A1) | 거래소 전송은 코어가 내보낸 "의도" 이벤트를 어댑터가 실행. ack/fill/reject/timeout은 전부 입력 이벤트로 저널을 거쳐 재진입 | 없음. 어댑터가 응답 없이 죽은 경우(보냈는지 모름)의 상태는 원문에 처방이 없음 |
| 결정성 | "The business logic is deterministic and very fast" (A1 각주 12) | 코어 금지 목록: 시계 읽기, 난수, 해시맵 반복 순서 의존, 부동소수, 스레드 타이밍. 시간·난수는 입력 이벤트 필드로 주입 | 시간 주입은 원문에 근거가 없다(§1.1 확인 불가). 채택 근거는 "입력만으로 상태가 유도된다"는 문장에서의 추론 |
| 변이 전 완전 검증 | "fully valid before doing any mutation" (A1) | 핸들러를 `validate(&State, &Event) -> Result<Plan>` → `apply(&mut State, Plan)`(실패 불가) 두 단계로 분리. apply의 패닉은 프로세스 중단으로 취급 | 없음. Rust 타입으로 강제 가능 |
| 스냅샷 + 저널 재생 | "loading a recent snapshot, and replaying a days worth of journals" (A1) | 스냅샷 = (저널 시퀀스 N, 상태 직렬화, 체크섬). tmp 작성 → fsync → rename → 디렉터리 fsync (Part 2 (h)) | LMAX는 야간 저부하 시간에 수행. 24/7 암호화폐 시장에는 그런 시간이 없어 스냅샷 중 지연 영향은 별도 측정 필요 |
| 출력은 저널하지 않고 재계산 | "there's no gain from storing the results" (A1 각주 12) | 재생 모드에서는 출력 방출을 억제. 그 위에 (a) 전송 의도에 결정적 client order id를 부여, (b) "전송 시도함" 사실을 저널에 기록, (c) 재시작 후 거래소 상태와 대사 | **원문 근거 없음.** 원문은 재생 시 외부 중복 전송 문제를 다루지 않는다. (a)–(c)는 전부 제안이며 at-least-once + 멱등 키에 의존 |
| 유계·사전 할당 링 버퍼 | "pre-allocated bounded data structure in the form of a ring-buffer" (A2, 3) | 유계 채널(`tokio::sync::mpsc` 또는 `crossbeam-channel`). 이벤트 타입은 힙 할당 없는 고정 크기 enum을 우선 | 범용 채널은 Disruptor의 다중 소비자 의존 그래프·배치 읽기를 주지 않음. 단일 소비자 구조라면 필요 없음 |
| 배압 | "we want to use a bounded queue implementation to ensure producers do not outpace consumers by creating back pressure" (A2, §5) | 채널이 가득 차면 입구에서 거절/대기. 거절 정책을 명시 | LMAX 입력 링은 2천만 슬롯. 작은 버퍼에서는 배압이 훨씬 자주 드러남 |
| Wait strategy | BusySpin/Yielding/Sleeping/Blocking (A4) | 기본은 blocking 수신. busy-spin은 측정으로 필요가 입증될 때만 | blocking은 A4 표현으로 "the slowest"이며 조건변수 지터가 생김. busy-spin은 코어 하나를 상시 점유하고 물리 코어 여유를 요구 |
| journaler 배칭 | "useful for flushing results to an IO device if necessary" (A2, 3.7) | 그룹 커밋: 대기 중인 이벤트를 모아 한 번 `write` + 한 번 `fdatasync` | 배치가 클수록 크래시 시 미내구 이벤트 수가 늘어남. (가) 게이트를 쓰면 미내구 이벤트는 아직 처리 전이므로 외부 영향 없음 |
| 캐시 친화 자료구조, 쓰레기 최소화 | "cache-friendly and careful with garbage" (A1) | 인덱스 기반 슬랩/`Vec`, 정수 키. 핫 패스 할당 회피 | GC 관련 근거는 Rust에 직접 적용되지 않음 |
| 성능은 시험으로 | "the best programmers prefer profilers and test cases to speculation" (A1) | 벤치마크와 지연 히스토그램을 먼저 만들고 최적화는 그 뒤 | — |
| 야간 재시작 | "restart the Business Logic Processors every night" (A1) | 주기적 재시작 리허설(스냅샷+재생 경로 검증) | LMAX는 복제 덕에 "no downtime". 단일 노드는 재시작이 곧 중단 |

---

## Part 2 — 내구성 프리미티브

### 2.0 호스트 관찰 (출처가 아닌 현장 값, 2026-10-02)

- `uname -r` = 7.0.0-34-generic, `rustc 1.96.0 (ac68faa20 2026-05-25)`
- 프로젝트 경로의 파일시스템: `/dev/nvme1n1p2 ext4 rw,relatime,stripe=32`
- `/proc/fs/ext4/nvme1n1p2/options`: `data=ordered`, `barrier`, `delalloc`, `auto_da_alloc`, `journal_checksum`, `commit=5`, `errors=continue`, `max_batch_time=15000`, `min_batch_time=0`
- `/sys/block/nvme1n1`: model `Samsung SSD 980 500GB`, `queue/write_cache = write back`, `queue/fua = 1`, logical/physical block 512/512
- 해석: 커널은 이 장치를 **휘발성 write cache가 있는 장치**로 인식한다(B20의 `BLK_FEAT_WRITE_CACHE`에 해당하는 상태). 장치가 FLUSH를 정직하게 수행하는지, PLP 유무는 이 값으로 알 수 없다. Samsung 980의 PLP에 관한 제조사 문서는 가져오지 않았다. **확인 불가**(소유자 전제를 그대로 사용).
- `errors=continue`: ext4 문서 표현으로 "Keep going on a filesystem error."(B19). 파일시스템 오류 시 read-only 재마운트가 **기본 동작이 아니다**.

### 2.1 질문별 답

#### (a) `write()` 반환 뒤 무엇이 살아남는가

- 원문: "A successful return from write() does not make any guarantee that data has been committed to disk." (B3, NOTES)
- 원문: "The only way to be sure is to call fsync(2) after you are done writing all your data." (B3, NOTES)
- 원문: "A write call only transfers data from application memory into the operating system" (B13, §1)
- 원문: "The storage device may further buffer the data in a volatile write-back cache. If power is lost while data is in this cache, the data will be lost." (B23)

| 사건 | 살아남는가 | 근거 |
|---|---|---|
| 프로세스 kill (SIGKILL, panic=abort) | 예 — 데이터가 이미 OS(page cache)에 있음 | **추론**: B13·B23의 계층 설명에서 도출. "프로세스가 죽어도 남는다"고 직접 말한 문장은 접근 범위에 없음 |
| OS 크래시/커널 패닉 | 보장 없음 | B3 NOTES. ext4 기본값에서는 메타데이터가 최대 5초치 유실될 수 있고(원문: "you will lose as much as the latest 5 seconds of metadata changes", B19), 원문: "due to delayed allocation even older data can be lost on power failure" (B19, commit=) |
| 전원 장애 | 보장 없음 | 위와 같음 + 장치 휘발성 캐시(B23) |

부분 쓰기: 원문: "Note that a successful write() may transfer fewer than count bytes." (B3). Rust 쪽도 같다. 원문: "A successful write() is allowed to write only part of the given data" (B9).

#### (b) fdatasync vs fsync, 파일 크기 변경

- fsync: 원문: "so that all changed information can be retrieved even if the system crashes or is rebooted. This includes writing through or flushing a disk cache if present." (B1)
- 원문: "The call blocks until the device reports that the transfer has completed." (B1)
- fdatasync: 원문: "does not flush modified metadata unless that metadata is needed in order to allow a subsequent data retrieval to be correctly handled" (B1)
- 크기 변경: 원문: "a change to the file size (st_size, as made by say ftruncate(2)), would require a metadata flush" (B1)
- 같은 구분이 open(2)에도 있다. 원문: "O_DSYNC would only guarantee to flush updates to the file length metadata" (B2, Synchronized I/O)

→ append로 파일이 커지는 저널에서 `fdatasync`는 mtime 등은 생략하되 **크기 메타데이터는 플러시해야 한다**고 문서가 말한다. 사전 할당으로 크기가 변하지 않으면 플러시할 메타데이터가 줄어든다는 것은 **추론**이다((g) 참조).

Rust 대응:

- `sync_all`: 원문: "Attempts to sync all OS-internal file content and metadata to disk." (B8)
- `sync_data`: 원문: "it might not synchronize file metadata to the filesystem" / "Note that some platforms may simply implement this in terms of sync_all." (B8)
- 문서는 syscall 대응을 적지 않는다. 1.96.0 소스(B11)에서 Linux는 `sync_all` → `libc::fsync`, `sync_data` → `libc::fdatasync`이고 둘 다 `cvt_r`로 감싸져 **EINTR이면 자동 재호출**된다. 구현 사실이지 문서화된 보장은 아니다.
- 원문: "Errors detected on closing are ignored by the implementation of Drop. Use the method sync_all if these errors must be manually handled." (B8)

#### (c) 생성/rename 뒤 부모 디렉터리 fsync

- 원문: "Calling fsync() does not necessarily ensure that the entry in the directory containing the file has also reached disk." (B1)
- 원문: "For that an explicit fsync() on a file descriptor for the directory is also needed." (B1)
- 원문: "An fsync() on a file does not guarantee that the file's directory entry is also persisted." (B14, §4.4 Safe file flush)
- 실제 사례: 원문: "ZooKeeper does not explicitly persist directory entries of log files, which can lead to lost data." (B14, §4.2)
- SQLite의 관행: 원문: "On Unix, the directory that contains the super-journal is also synced in order to make sure the super-journal file will appear in the directory following a power failure." (B15, §5)
- 반대 방향 단서: 원문: "Most file systems, however, persist directory entries that the file is dependent on" (B14, §4.4) — 동작은 그럴 수 있어도 보장은 아니다.

→ 답: 필요하다. 새 저널 세그먼트 생성 직후와 스냅샷 rename 직후에 디렉터리 fd를 fsync한다.

#### (d) 크래시 뒤 torn/garbage 꼬리, 그리고 순서 역전

**ext4 data=ordered의 문서상 의미**

- 원문: "All data are forced directly out to the main file system prior to its metadata being committed to the journal." (B19, data=ordered)
- 대조: data=writeback은 원문: "can potentially leave stale data exposed in recently written files in case of an unclean shutdown" (B19)

**Pillai 2014의 실측 (ext4-ordered 열, Table 1 — 조사자의 이미지 판독)**

| 성질 | ext4-ordered에서 위반 사례(×)가 있었는가 |
|---|---|
| Single sector append 원자성 | 없음 |
| Single block append 원자성 | 없음 |
| Multi-block append/writes 원자성 | **있음** (16개 구성 전부) |
| Multi-block prefix append | 없음 |
| Single block overwrite 원자성 | **있음** |
| Append → Append (같은 파일) 순서 | 없음 |
| Append → Any op 순서 | **있음** (ext4-nodelalloc에서는 없음) |
| Overwrite → Any op 순서 | **있음** |
| Directory op 원자성 / Dir op → Any op | 없음 / 없음 |

- 원문: "Current file systems do not provide atomic multi-block appends; appends can be broken down into multiple operations." (B14, §2.2.1)
- 원문: "most file systems seemingly guarantee that some prefix of the data written (e.g., the first 10 blocks of a larger append) will be appended atomically" (B14, §2.2.1)
- 원문: "Even with delayed allocation, successive appends to the same file are persisted in order." (B14, §2.2.2)
- 원문: "On file systems with the delayed allocation feature, it may be persisted after other operations." (B14, §2.2.2 — append가 **다른 파일/연산**보다 늦게 영속될 수 있음)
- garbage: 원문: "A crash can result in the appended portion of the file containing garbage" (B14, §4.2, LevelDB 사례 — 파일시스템 구성에 따라 노출)
- 원문: "only the current implementation of delayed allocation (where file size does not increase until actual content is persisted) works" (B14, §4.4.2)
- 한계: 원문: "Our testing is not complete, but finds persistence properties that do not hold" (B14, §2) — ×가 없다는 것은 "반례를 못 찾았다"이지 "보장된다"가 아니다.
- 원문: "it is risky to assume that any particular property will be supported by all file systems" (B14, §2.3)

**SQLite의 비관적 가정**

- 원문: "SQLite assumes that when a file grows in length that the new file space originally contains garbage and then later is filled in with the data actually written." (B15, §2)
- WAL은 프레임마다 체크섬과 salt로 유효 범위를 판정한다. 원문: "Checksums and counters attached to each frame are used to determine which frames within the WAL are valid" (B18, §4)
- 장치 수준: 원문: "We are told that in some flash memory controllers the wear-leveling logic can cause random filesystem damage if power is interrupted during a write." (B16, §4.1)
- 원문: "Corrupt data might also be introduced into an SQLite database by bugs in the operating system or disk controller; especially bugs triggered by a power failure." (B15, §9.4)

→ 답
- **꼬리가 부분적으로만 기록될 수 있는가**: 그렇다. 다중 블록 append는 원자적이지 않다(B14). 프로세스 kill로도 `write()`가 부분만 반영된 꼬리가 생길 수 있다(B3 partial write).
- **꼬리에 garbage가 올 수 있는가**: ext4 data=ordered + delalloc에서는 2014년 연구가 반례를 찾지 못했고 문서상 의미(데이터가 메타데이터 커밋보다 먼저)도 이를 뒷받침한다. 그러나 보장으로 문서화된 것은 아니며, PLP 없는 SSD의 장치 수준 손상은 파일시스템 보장 밖이다(B15 §9.4, B16 §4.1). SQLite처럼 **garbage가 올 수 있다고 가정**하고 레코드별 길이+체크섬으로 유효 접두부만 받아들이는 것이 출처들과 일치하는 방어다.
- **뒤의 append가 앞의 데이터보다 먼저 나타날 수 있는가(같은 파일)**: ext4-ordered에서 2014년 연구는 반례를 찾지 못했다("successive appends to the same file are persisted in order"). 현재 커널 7.0에서의 재검증 자료는 접근 범위에 없다. **확인 불가**. 서로 다른 파일 사이(예: 저널 세그먼트와 스냅샷)에서는 순서가 보장되지 않는다(Append → Any op ×).

#### (e) fsync가 오류를 반환했을 때

- 원문: "Since Linux 4.13, errors from write-back will be reported to all file descriptors that might have written the data which triggered the error." (B1, ERRORS EIO)
- 원문: "all three file systems mark pages clean after fsync fails, rendering techniques such as application-level retry ineffective" (B13, §1)
- ext4 ordered: 원문: "future calls to fsync never retry previous data writes that may have failed" (B13, §3.3.1)
- 커널 버전별(B12, Research notes):
  - Linux < 4.13: 원문: "fsync() errors can be lost in various ways; also buffers are marked clean after errors, so retrying fsync() can falsely report success"
  - Linux 4.13/4.15: 원문: "fsync() only reports writeback errors that occurred after you called open()"
  - Linux 4.14 및 ≥ 4.16: 원문: "you still only get the error once (so retrying fsync() is not OK)" / "buffers are still thrown away (either immediately or on memory pressure, depending on choice of fs)"
- PostgreSQL의 대응: 원문: "As of this PostgreSQL 12 commit, PostgreSQL will now PANIC on fsync() failure." (B12; 11, 10, 9.6, 9.5, 9.4로 백패치)
- page cache를 믿지 말 것: 원문: "applications should ensure recovery protocols only use existing persistent (on-disk) state to recover" (B13, §1)
- ext4의 오류 옵션은 기대대로 동작하지 않았다. 원문: "ext4 currently does not behave as expected with mount options data_err=abort and errors=remount-ro" (B13, §4; 커널 5.2.11 시험)
- B13 Table 1의 ext4 ordered 행: 실패 후 페이지 clean, 메모리 내용≠디스크, 오류 보고 immediate, journal 블록 실패 시 remount-ro, 파일에 non-overwritten block이 "anywhere", fsck로 탐지 불가.
- write(2)도 지연 오류를 보고할 수 있다. 원문: "Since Linux 4.13, errors from write-back come with a promise that they may be reported by subsequent. write() requests, and will be reported by a subsequent fsync(2)" (B3, EIO; 마침표 위치는 원문 그대로)

→ 답: **재시도는 안전하지 않다.** EIO/ENOSPC 등 fsync 오류는 "이 fd로 쓴 것 중 무언가가 디스크에 없다"로 해석하고 프로세스를 중단한 뒤, 재시작 시 디스크에서 다시 읽어 체크섬으로 유효 접두부를 정하는 것이 B12·B13과 일치한다. 주의: Rust std는 EINTR에 한해 fsync를 자동 재호출한다(B11). EINTR 재호출이 Linux에서 오류 상태를 잃지 않는다는 근거는 접근 범위에 없다. **확인 불가**.

#### (f) 휘발성 write cache / FLUSH / PLP 없는 소비자 SSD

- 원문: "This explicitly guarantees that previously completed write requests are on non-volatile storage before the flagged bio starts." (B20, Explicit cache flushes — `REQ_PREFLUSH`)
- 원문: "I/O completion for this request is only signaled after the data has been committed to non-volatile storage" (B20, Forced Unit Access)
- 원문: "Write barriers enforce proper on-disk ordering of journal commits, making volatile disk write caches safe to use, at some performance penalty." (B19, barrier=)
- 원문: "If your disks are battery-backed in one way or another, disabling barriers may safely improve performance." (B19) — 뒤집으면 PLP 없는 장치에서 `nobarrier`는 안전하지 않다.
- 원문: "This system call does not flush disk write caches and thus does not provide any data integrity on systems with volatile disk write caches." (B6, sync_file_range Warning)
- 오래된 커널에 대한 단서: 원문: "The fsync() implementations in older kernels and lesser used filesystems do not know how to flush disk caches." (B1, HISTORY)
- 원문: "Unfortunately, most consumer-grade mass storage devices lie about syncing." (B16, §3.1)
- 원문: "There is no way for SQLite to detect that either is lying and that writes might be occurring out-of-order." (B16, §3.1)
- 원문: "It is best to assume a volatile cache, and program defensively." (B23)
- sync가 장벽으로만 동작해도 얻는 것: 원문: "A sync failure during a COMMIT might result in loss of durability but not in a corrupt database file." (B16, §3.1, WAL 모드 SQLite에 대한 서술)

→ 답: 소프트웨어 스택(ext4 barrier + fsync/fdatasync → FLUSH/FUA)은 휘발성 캐시를 비우도록 설계돼 있고, 이 호스트에서 barrier는 켜져 있다(H1). 남는 것은 **장치가 FLUSH 완료를 정직하게 보고하는가**이며, 소프트웨어로 검출할 방법이 없다고 B16이 말한다. Samsung 980에 대한 실측 자료는 가져오지 못했다. **확인 불가** — 전원 차단 시험으로만 확인할 수 있다.

#### (g) 사전 할당(fallocate)이 그림을 바꾸는가

- 원문: "After a successful call, subsequent writes into the range specified by offset and size are guaranteed not to fail because of lack of disk space." (B5)
- 원문: "Any subregion within the range specified by offset and size that did not contain data before the call will be initialized to zero." (B5)
- 원문: "The file size (as reported by stat(2)) will be changed if offset+size is greater than the file size." (B5, mode 0)
- `FALLOC_FL_KEEP_SIZE`: 원문: "Preallocating zeroed blocks beyond the end of the file in this manner is useful for optimizing append workloads." (B5)
- 경고: 원문: "When writing into preallocated space, many filesystems also require calls into the block allocator, which this system call does not sync out to disk." (B6 — `sync_file_range`에 대한 문장이지만, 사전 할당 영역 쓰기에도 메타데이터 갱신이 따른다는 사실을 알려 준다)
- Rust `set_len`은 fallocate가 아니다. 원문: "the file will be extended to size and have all of the intermediate data filled in with 0s" (B8). 1.96 소스에서 `ftruncate64` 호출(B11) — 블록을 예약한다는 문장은 없다.

→ 답(문서로 확인되는 부분): ENOSPC가 쓰기 시점이 아니라 할당 시점으로 당겨지고, 미기록 영역은 0으로 읽힌다.

→ **추론**(문서가 직접 말하지 않음, 미검증):
1. mode 0으로 크기까지 늘려 두면 이후 쓰기는 파일시스템 관점에서 append가 아니라 **파일 크기 내부의 쓰기**가 된다. B14의 "같은 파일 append 순서" 성질은 크기를 늘리는 쓰기에 대한 것이므로 그대로 옮겨진다고 볼 근거가 없다. B14 Table 1에서 ext4-ordered는 overwrite 원자성·순서에 ×가 있다. 따라서 fdatasync 없이 연달아 쓴 레코드 N, N+1 중 N+1만 영속되는 경우를 배제할 수 없다.
2. 꼬리는 EOF가 아니라 **0으로 채워진 영역**으로 끝난다. 리더는 "길이 0/체크섬 불일치 = 유효 저널의 끝"으로 판정해야 하고, 0만으로 이뤄진 레코드가 유효하지 않도록 프레임을 설계해야 한다. B14: "Filling the appended portion with zeros instead of garbage still causes failure"(§4.4.2)는 이를 처리하지 못한 애플리케이션 사례다.
3. `KEEP_SIZE`를 쓰면 크기는 계속 늘어나므로 fdatasync가 크기 메타데이터를 플러시해야 하는 것은 (b)와 같다.
4. unwritten extent → written 전환 메타데이터가 "subsequent data retrieval"에 필요한 메타데이터로서 fdatasync에 포함되는지는 man page가 명시하지 않는다. **확인 불가**.

#### (h) rename은 원자적인가, rename + 디렉터리 fsync가 스냅샷 파일의 통용 패턴인가

- 원문: "If newpath already exists, it will be atomically replaced, so that there is no point at which another process attempting to access newpath will find it missing." (B4)
- 이 문장은 **동시 실행 중인 다른 프로세스**에 대한 원자성이다. rename(2)에는 크래시 후 상태에 대한 문장이 없다.
- 크래시 관점: 원문: "Directory operations such as rename() and link() are seemingly atomic on all file systems that use techniques like journaling or copy-on-write for consistency." (B14, §2.2.1 — "seemingly")
- ext4의 보정: 원문: "the data blocks of the new file are forced to disk before the rename() operation is committed" (B19, auto_da_alloc). 같은 문단은 fsync 없이 rename하는 프로그램을 "broken applications"라 부른다.
- 통용 절차(B23): "create a new temp file (on the same file system!)" → "write data to the temp file" → "fsync() the temp file" → "rename the temp file to the appropriate name" → "fsync() the containing directory"
- Rust: 원문: "This function currently corresponds to the rename function on Unix" / "This will not work if the new name is on a different mount point." (B10)

→ 답: 그렇다. tmp 작성 → 파일 fsync → rename → 디렉터리 fsync가 접근한 출처들이 일치해서 제시하는 패턴이다. 크래시 원자성은 "저널링 파일시스템에서 그렇게 보인다"(B14)는 실측 수준이며 POSIX 문서의 보장은 아니다. 스냅샷 리더는 그래도 파일 전체 체크섬을 검증해야 한다(제안).

### 2.2 그 밖의 발췌

- `O_APPEND`: 원문: "The modification of the file offset and the write operation are performed as a single atomic step." (B2). Rust: 원문: "Append mode guarantees that writes will be positioned at the current end of file, even when there are other processes or threads appending to the same file." (B9)
- `O_DSYNC`: 원문: "as though each write(2) was followed by a call to fdatasync(2)" (B2). `O_SYNC`는 fsync(2)에 대응.
- `O_DIRECT`: 원문: "The O_DIRECT flag on its own makes an effort to transfer data synchronously, but does not give the guarantees of the O_SYNC flag that data and necessary metadata are transferred." (B2). 정렬 제약이 있고 직접 I/O 오류 시 원문: "the data at the file offset on which the write() was attempted should be considered inconsistent" (B3, NOTES).
- `sync_file_range`: 원문: "This system call is extremely dangerous and should not be used in portable programs." / "None of these operations writes out the file's metadata." (B6)
- ext4 `journal_checksum`: 원문: "Enable checksumming of the journal transactions." (B19) — **파일시스템 저널**의 체크섬이며 애플리케이션 데이터 블록을 보호하지 않는다(뒤 절반은 추론).
- ext4 `data_err=ignore(*)`: 원문: "Just print an error message if an error occurs in a file data buffer." (B19, 기본값)
- ext4 `max_batch_time`: 원문: "we wait for a small amount of time to see if any other transactions can piggyback on the synchronous write" (B19). 기본 15000us, 호스트도 15000(H1). 단일 writer의 fsync 지연에 영향을 줄 수 있다(추론, 미측정).
- errseq_t(2차, B22): 원문: "it makes it possible to report a writeback error exactly once to every process that calls fsync() on that file"
- flock: 원문: "Locks created by flock() are associated with an open file description" (B7). advisory다: "flock() places advisory locks only" (B7).

### 2.3 표: 저널이 의존하는 보장 → 출처/인용 → 남는 가정 → 검증 수단

"프로세스 kill 시험"은 실행 중인 프로세스를 SIGKILL하고 재시작하는 시험을 뜻한다. kill은 page cache를 잃지 않으므로 fsync 유무·장치 캐시와 관련된 성질은 원리상 구별할 수 없다.

| # | 저널이 의존하는 보장 | 출처/인용 | 남는 가정 | kill 시험으로 검증 가능한가 | 전원 장애 시험 필요 여부 |
|---|---|---|---|---|---|
| 1 | `write()`가 반환한 바이트는 프로세스가 죽어도 남는다 | B13 "only transfers data from application memory into the operating system"; B3 NOTES | 직접 문장 없음(추론). 커널 자체는 생존해야 함 | 예 | 아니오 |
| 2 | `fdatasync` 성공 = 그 시점까지의 데이터와 크기 메타데이터가 안정 저장소에 있다 | B1 "can be retrieved even if the system crashes or is rebooted"; "would require a metadata flush" | 장치가 FLUSH를 정직하게 수행(f). barrier가 켜져 있음(H1 확인) | 아니오 | **예** (유일한 검증 수단) |
| 3 | 부분 쓰기는 호출자가 이어서 써야 한다 | B3 "may transfer fewer than count bytes"; B9 | `write_all` 계열 사용. tokio `write_all`은 취소 안전하지 않음(Part 3) | 예 | 아니오 |
| 4 | 새 세그먼트/스냅샷 파일은 디렉터리 fsync 뒤에야 이름이 내구적이다 | B1 "an explicit fsync() on a file descriptor for the directory is also needed" | — | 아니오 | 예 (또는 블록 수준 크래시 주입 도구) |
| 5 | 같은 파일의 연속 append는 순서대로 영속된다 | B14 "successive appends to the same file are persisted in order" | 2014년 실측. 커널 7.0 재검증 없음. 사전 할당 시 적용 여부 불명(g) | 아니오 | 예 |
| 6 | 꼬리의 불완전·손상 레코드는 레코드별 길이+체크섬으로 검출해 잘라낸다 | B15 "originally contains garbage"; B18 프레임 유효성; B14 "do not provide atomic multi-block appends" | 체크섬 충돌 확률; 0으로 채워진 꼬리 처리(g); 중간 레코드 손상은 "끝"이 아니라 오류로 구분 | 부분적으로 예 (kill로 생기는 잘린 레코드까지. 블록 단위 torn/garbage는 인위적 파일 변조 시험으로 대체) | 실제 발생 형태 확인에는 예 |
| 7 | fsync 오류 후에는 재시도하지 않고 중단, 디스크에서 재구성 | B13 "rendering techniques such as application-level retry ineffective"; B12 "retrying fsync() is not OK"; "PANIC on fsync() failure" | 재시작 시 page cache가 아니라 디스크 내용을 읽게 됨을 보장할 수 없음(B13: 페이지가 clean으로 남아 메모리≠디스크). std의 EINTR 자동 재호출 | 아니오 | 아니오 — **오류 주입**(dm-flakey/dm-error류 장치, 또는 I/O 계층 추상화에 가짜 구현)이 필요 |
| 8 | 휘발성 write cache는 FLUSH로 비워진다 | B20 "previously completed write requests are on non-volatile storage"; B19 barrier | 소비자 SSD가 거짓 보고하지 않음(B16 "lie about syncing"). Samsung 980 실측 없음 | 아니오 | **예** |
| 9 | 사전 할당 영역 쓰기는 ENOSPC로 실패하지 않는다 | B5 "guaranteed not to fail because of lack of disk space" | ext4에서 extent 전환 메타데이터용 저널 공간 등 예외 여부는 미확인 | 예 (디스크를 채운 뒤 쓰기) | 아니오 |
| 10 | 사전 할당 후 미기록 영역은 0으로 읽힌다 | B5 "will be initialized to zero" | 크래시 후에도 그렇다는 직접 문장 없음. extent 전환이 fdatasync에 포함되는지 불명 | 정상 경로는 예 | 예 |
| 11 | rename은 대상 이름을 원자적으로 교체한다 | B4 "atomically replaced"; B14 "seemingly atomic"; B23 절차 | 크래시 원자성은 실측 수준. 디렉터리 fsync 전에는 옛 이름/새 이름 어느 쪽도 가능 | 동시 관측 원자성은 예, 크래시 원자성은 아니오 | 예 |
| 12 | `O_APPEND` 쓰기는 offset 이동과 쓰기가 한 단계다 | B2 "single atomic step" | 단일 writer이므로 사실상 불필요한 보장 | 예 | 아니오 |
| 13 | 단일 인스턴스 보장(락 파일) | B7 flock, B8 `File::lock` "corresponds to the flock function on Unix" | advisory. 락은 fd가 닫히면 풀림 | 예 (kill 뒤 락 해제와 재획득) | 아니오 |
| 14 | ext4 data=ordered는 크기 커밋 전에 데이터를 내보내 stale 데이터 노출을 막는다 | B19 "prior to its metadata being committed to the journal" | 마운트 옵션이 바뀌지 않음(H1 현재 ordered). 다른 파일시스템으로 옮기면 무효 | 아니오 | 예 |
| 15 | 장치가 요청 범위 밖의 블록을 손상시키지 않는다 | B15 "powersafe overwrite" 가정(SQLite의 가정으로 서술됨) | PLP 없는 플래시 컨트롤러에서 반례 보고(B16 §4.1) | 아니오 | 예 (그래도 완전 검증은 불가) |

---

## Part 3 — Rust 생태계 후보 (2026-10-02 기준)

읽는 법: "최신 안정판 (날짜)"는 crates.io API의 `max_stable_version`과 그 버전의 `created_at`. MSRV는 crates.io의 `rust_version` 필드(없으면 "미표기"). RustSec은 advisory-db `117edb3` 기준이며 "없음"은 해당 crate 디렉터리가 없다는 뜻이다. 표에 실린 crate의 MSRV는 모두 Rust 1.96 이하다.

### 3.1 런타임 / 채널

| crate | 최신 안정판 (날짜) | 라이선스 | MSRV | RustSec | 의존하려는 문서화된 보장 / 메모 |
|---|---|---|---|---|---|
| tokio | 1.53.1 (2026-07-20) | MIT | 1.71 | 과거 5건(최근 RUSTSEC-2025-0023, broadcast 채널 unsound). 모두 ≥1.44.2에서 패치 | 아래 "tokio 취소 안전성" 참조 |
| crossbeam-channel | 0.5.17 (2026-09-05) | MIT OR Apache-2.0 | 1.60 | 3건(최근 RUSTSEC-2025-0024 "double free on Drop", ≥0.5.15 패치) | 유계: "This channel has a buffer that can hold at most cap messages at a time." `send`: "If the channel is full and not disconnected, this call will block until the send operation can proceed." |
| rtrb | 0.4.0 (2026-08-17) | MIT OR Apache-2.0 | 1.38 | RUSTSEC-2026-0274 (2026-08-04, `ReadChunk::commit` double free/UAF, 0.3.5·0.4.0에서 패치) | SPSC 전용. "Reading from and writing into the ring buffer is lock-free and wait-free." / "there is no way for the reading thread to wait for new data, other than trying repeatedly until reading succeeds" → 대기 전략이 사실상 busy-poll |
| disruptor | 4.4.0 (2026-08-08) | MIT | 미표기 | 없음 | Rust 포트(nicholassm/disruptor-rs). 대기 전략 export는 `BusySpin`, `BusySpinWithSpinLoopHint`, `Sleep`뿐 — **blocking 전략 없음**. "The Disruptor in this library can only be used once. I.e. it cannot be rewound and restarted." README: "This library needs to use Unsafe to achieve low latency." / "All tests are run on Miri in CI/CD." / "Verification in TLA+". 단일 메인테이너 저장소(별 906), 버전 36개 |
| flume | 0.12.0 (2025-12-08) | Apache-2.0/MIT | 미표기 | 없음 | "Safe: No unsafe code anywhere in the codebase!" MPMC, sync/async 혼용 |
| kanal | 0.1.1 (2025-03-23) | MIT | 미표기 | 없음 | 자체 튜닝 mutex 사용("Kanal utilizes a specially tuned mutex"). 0.1.x |

**tokio 취소 안전성 (tokio 1.53.1 docs.rs)**

- `mpsc::Receiver::recv`: 원문: "This method is cancel safe. If recv is used as a branch in tokio::select! and another branch completes first, it is guaranteed that no messages were received on this channel."
- `mpsc::Sender::send`: 원문: "it is guaranteed that the message was not sent. However, in that case, the message is dropped and will be lost."
- 대안: 원문: "To avoid losing messages, use reserve to reserve capacity, then use the returned Permit to send the message." 단, 원문: "Cancelling a call to reserve makes you lose your place in the queue."
- 유계 배압: 원문: "Once the buffer is full, attempts to send new messages will wait until a message is received from the channel." (`mpsc::channel`)
- 순서: 원문: "All data sent on Sender will become available on Receiver in the same order as it was sent."
- `select!`: 취소 안전 목록에 `mpsc::Receiver::recv`, `AsyncWriteExt::write`가 있고, **안전하지 않은** 목록에 `AsyncReadExt::read_exact`, `read_to_end`, `read_to_string`, `AsyncWriteExt::write_all`이 있다. 정의: 원문: "If you have a future that has not yet completed, then it must be a no-op to drop that future and recreate it."
- `AsyncWriteExt::write_all`: 원문: "This method is not cancel safe." / "the provided buffer may have been partially written, but future calls to write_all will start over from the beginning of the buffer"
- `tokio::fs`: 원문: "Tokio will use ordinary blocking file operations behind the scenes. This is done using the spawn_blocking threadpool" / "calls to write will return before the write has finished, and flush will wait for the write to finish"
- 동기↔비동기 경계: 원문: "in synchronous code you will need to use the blocking_send or blocking_recv methods" (mpsc 모듈 문서)

**제안 (런타임/채널)**: I/O 가장자리(거래소 어댑터, 대시보드)는 tokio 1.53.1, 코어와 journaler는 **전용 OS 스레드 + `std::fs::File`**. 스레드 사이는 `tokio::sync::mpsc` 유계 채널 하나로 통일하고 비동기 쪽은 `reserve()`→`Permit::send`, 동기 쪽은 `blocking_recv`를 쓴다.
- 이유: 저널 쓰기를 `tokio::fs`로 하면 문서대로 `spawn_blocking` 뒤에서 일어나고 `write_all`은 취소 시 부분 기록이 남는다 — 저널에는 받아들일 수 없는 성질이다. `send`를 `select!` 분기에 두면 메시지가 버려진다. 채널 crate를 하나 더 들이지 않아도 된다.
- 주요 위험: blocking 수신은 LMAX 문서 표현으로 가장 느린 대기 방식이다. 지연 요구가 측정으로 확인되면 그때 crossbeam-channel 0.5.17이나 rtrb로 바꾼다. `disruptor` crate는 blocking 전략이 없어 유휴 시 코어 하나를 계속 태우고, unsafe 기반·단일 메인테이너이므로 측정 근거 없이는 채택하지 않는다.

### 3.2 저널 직렬화

| crate | 최신 안정판 (날짜) | 라이선스 | MSRV | RustSec | 형식 안정성 / 메모 |
|---|---|---|---|---|---|
| serde | 1.0.229 (2026-07-18) | MIT OR Apache-2.0 | 1.56 | 없음 | 프레임워크. 형식 아님 |
| **bincode** | 3.0.0 (2025-12-16) | MIT | 1.85.0 | **RUSTSEC-2025-0141 "Bincode is unmaintained"** (2025-12-16, patched 없음) | **사용 불가.** 3.0.0 README: "Due to a doxxing and harassment incident, development on bincode has ceased. No further releases will be published on crates.io." 3.0.0의 `src/lib.rs`는 `compile_error!("https://xkcd.com/2347/");` 한 줄(직접 확인). 권고문: "The team considers version 1.3.3 a complete version of bincode that is not in need of any updates." 직전 실사용 버전은 2.0.1 (2025-03-10). 권고문이 든 대안: wincode, postcard, bitcode, rkyv |
| postcard | 1.1.3 (2025-07-24) | MIT OR Apache-2.0 | 미표기 | 없음 | "As of v1.0.0, postcard has a documented and stable wire format." 사양: "Breaking changes to the wire format would be considered a breaking change to the library" (v2.0.0 필요). 비자기기술 형식이며 "Backwards/forwards compatibility between revisions of a postcard schema are considered outside of the scope of the postcard wire format". 정수는 varint. 저장소 최근 푸시 2026-09-21 |
| rkyv | 0.8.18 (2026-08-05) | MIT | 1.81 | 6건. 2026년에만 5건(0001, 0122, 0233 "Crafted archives can cause a use-after-free during deserialization", 0234, 0235). 전부 ≥0.8.17에서 패치 | zero-copy. "Enabling and disabling these features may change rkyv's serialized format, and as such can cause previously-serialized data to become unreadable." 0.x 버전 간 형식 안정성 문장은 접근 범위에서 **확인 불가** |
| zerocopy | 0.8.59 (2026-09-25) | BSD-2-Clause OR Apache-2.0 OR MIT | 1.56.0 | 1건(2023-0074, 패치됨) | 고정 레이아웃 구조체 ↔ 바이트 변환(`FromBytes`/`IntoBytes`). 형식은 사용자가 `repr`로 정의 |
| bytes | 1.12.1 (2026-07-08) | MIT | 1.57 | RUSTSEC-2026-0007 (`BytesMut::reserve` 정수 오버플로, ≥1.11.1 패치) | 버퍼 타입. tokio/hyper가 어차피 끌어옴 |
| rmp-serde | 1.3.1 (2025-12-23) | MIT | 1.85 | 1건(2022-0092, ≥1.1.1 패치) | 기본은 구조체를 **배열**로 인코딩: "This is the default MessagePack serialization mechanism, emitting the most compact representation." `to_vec_named`: "Resulting MessagePack message will contain field names". Hyperliquid 공식 Rust SDK(master `aac75585daf1`, `src/exchange/exchange_client.rs`)는 `rmp_serde::to_vec_named(self)` 결과에 timestamp(be)·vault 표식을 붙여 `keccak256`한다(직접 확인) |
| serde_json | 1.0.151 (2026-07-20) | MIT OR Apache-2.0 | 1.71 | 없음 | 거래소 API, 대시보드 |
| (참고) wincode | 0.6.2 (2026-09-28) | Apache-2.0 | 1.89.0 | 없음 | "Fast bincode de/serialization with placement initialization" — bincode 호환 대안으로 권고문에 등장. 문서 미정독 |

**제안 (직렬화)**: 저널 프레임(매직/버전, 길이, 체크섬, 시퀀스)은 **직접 정의한 고정 헤더**로 쓰고, 페이로드는 postcard 1.1.3으로 직렬화하되 레코드마다 스키마 버전 태그를 둔다. rmp-serde 1.3.1은 Hyperliquid 액션 해싱에만, serde_json은 API 경계에만 쓴다. bincode는 금지 목록에 올린다(`cargo-deny`의 bans).
- 이유: postcard는 접근한 후보 중 유일하게 wire format 안정성을 문서로 약속한다. rkyv는 올해 조작된 아카이브 관련 권고가 다수였고 검증 계층이 필요하다 — 저널 파일은 손상 입력이 될 수 있는 데이터다.
- 주요 위험: postcard는 스키마 진화를 책임지지 않는다. enum 변형 순서나 필드 추가가 조용히 옛 저널을 못 읽게 만든다 → 골든 파일 호환 시험이 필수. 최신 릴리스가 2025-07이어서 릴리스 주기는 느리다.
- Hyperliquid 서명은 필드 **순서와 이름**이 바이트 단위로 맞아야 한다. `to_vec`(배열)이 아니라 `to_vec_named`(맵)임을 SDK 소스로 확인했다. Hyperliquid 공식 문서 자체는 이번에 가져오지 않았다.

### 3.3 체크섬

| crate | 최신 안정판 (날짜) | 라이선스 | MSRV | RustSec | 알고리즘 / 하드웨어 가속 (문서 표현) |
|---|---|---|---|---|---|
| crc32c | 0.6.8 (2024-06-09) | Apache-2.0/MIT | 미표기 | 없음 | CRC-32C(Castagnoli). "It provides both a software implementation, and a hardware-optimized one for SSE 4.2." 런타임 cpuid 감지 |
| crc32fast | 1.5.2 (2026-09-12) | MIT OR Apache-2.0 | 1.63 | 없음 | "Fast, SIMD-accelerated CRC32 (IEEE) checksum computation." sse/pclmulqdq, Rust 1.89+에서 vpclmulqdq(avx2/avx512), aarch64 crc32. 런타임 감지 |
| crc-fast | 1.10.0 (2025-12-31) | MIT OR Apache-2.0 | 1.89 | 없음 | "Hardware-accelerated CRC calculation for all known CRC-32 and CRC-64 variants" |
| crc64fast-nvme | 1.2.1 (2025-11-14) | MIT OR Apache-2.0 | 1.70.0 | 없음 | CRC-64/NVME 단일 변형. crc-fast 문서가 선행 crate로 언급 |
| xxhash-rust | 0.8.19 (2026-09-28) | BSL-1.0 | 미표기 | 없음 | "Implementation of xxHash in Rust". xxh32/xxh64/xxh3는 feature로 선택 |
| blake3 | 1.8.7 (2026-08-20) | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | 미표기 | 없음 | "The official Rust implementation of the BLAKE3 cryptographic hash function." |

무엇을 검출하는가(일반 지식 — 가져온 crate 문서에는 검출 능력에 대한 서술이 없다): CRC와 xxHash는 **우발적** 손상(잘린 쓰기, 비트 오류) 검출용이고 의도적 변조에는 방어가 되지 않는다. BLAKE3는 암호학적 해시다. CRC는 짧은 버스트 오류에 대한 수학적 검출 보장이 있고 xxHash는 그런 보장이 없는 해시라는 구분도 이번 조사에서 출처로 확인하지 않았다.

**제안 (체크섬)**: 레코드별 체크섬은 crc32fast 1.5.2(가장 최근까지 유지보수, SIMD 런타임 감지), 스냅샷 파일 전체 무결성은 blake3 1.8.7 또는 같은 CRC.
- 이유: 의존성 하나로 충분하고, CRC-32(IEEE)는 외부 도구로도 재현하기 쉽다.
- 주요 위험: 32비트는 레코드 수가 많아지면 우연 일치 확률이 0이 아니다. 시퀀스 번호·길이·버전을 체크섬 범위에 넣고, 0으로 채운 영역이 유효 레코드로 해석되지 않게 한다(Part 2 (g)). crc32c는 SSE4.2 전용 명령을 쓰지만 최신 릴리스가 2024-06이다.

### 3.4 테스트

| crate/도구 | 최신 안정판 (날짜) | 라이선스 | MSRV | RustSec | 할 수 있는 것 / 없는 것 (문서 표현) |
|---|---|---|---|---|---|
| proptest | 1.11.0 (2026-03-24) | MIT OR Apache-2.0 | 1.85 | 없음 | "Hypothesis-like property-based testing and shrinking." |
| quickcheck | 1.1.0 (2026-02-10) | Unlicense OR MIT | 1.85.0 | 없음 | "Automatic property based testing with shrinking." |
| bolero | 0.13.6 (2026-09-30) | MIT | 1.68.0 | 없음 | "fuzz and property testing front-end" |
| loom | 0.7.2 (2024-04-23) | MIT | 1.65 | 없음 | C11 메모리 모델 아래 인터리빙을 순열 탐색. **한계**: "Other sources of non-determinism like random number generation or system calls cannot be modeled directly by loom, and must be mocked to be testable by loom." / "Any code that does not use loom's replacement types is invisible to loom". 큰 모델은 `LOOM_MAX_PREEMPTIONS`로 잘라야 함. 릴리스는 2024-04가 마지막(저장소 푸시 2026-02) |
| shuttle | 0.9.5 (2026-10-01) | Apache-2.0 | 미표기 | 없음 | 무작위 스케줄 탐색. "Shuttle is not sound (a passing Shuttle test does not prove the code is correct), but it scales to much larger test cases than Loom." |
| turmoil | 0.7.2 (2026-04-24) | MIT | 미표기 | 없음 | 0.7.2 문서: 단일 스레드에서 여러 호스트를 돌리고 "changes in the simulated network"로 장애 주입. 저장소 main README는 `turmoil-fs`(시뮬레이션 파일시스템, torn write)를 소개하지만 **crates.io에 `turmoil-fs`는 없다**(API: "does not exist"). `turmoil-net` 0.1.0 (2026-05-04)만 있음 |
| madsim | 0.2.34 (2025-10-11) | Apache-2.0 | 미표기 | 없음 | tokio 대체 결정적 런타임. "All I/O-related interfaces must be mocked during the simulation". tokio를 `madsim-tokio`로 교체하고 일부 crate를 git 패치해야 함 |
| cargo-nextest | 0.9.146 (2026-09-21) | Apache-2.0 OR MIT | 1.91 | 없음 | 테스트 러너. miri README: doctest 미지원 |
| cargo-mutants | 27.1.0 (2026-06-02) | MIT | 1.88 | 없음 | "Inject bugs and see if your tests catch them" |
| cargo-fuzz | 0.13.2 (2026-06-09) | MIT OR Apache-2.0 | 미표기 | 없음 | libFuzzer. (`libfuzzer-sys` 0.4.13, `arbitrary` 1.4.2) |
| miri | rustup 컴포넌트(nightly). README는 master 기준 | — | — | — | UB·일부 데이터 레이스 검출. 격리: "By default, Miri ensures a fully deterministic execution by isolating the program from the host system." 파일 I/O: "basic file system access) but most have not: for example, Miri currently does not support networking". 실제 파일에 접근하려면 `-Zmiri-disable-isolation`. `fsync`/`fdatasync`/`fallocate`/`flock` 지원 여부는 README에 목록이 없어 **확인 불가**. 스케줄 탐색은 불완전: "that will still by far not explore all possible executions" |
| cargo-llvm-cov | 0.9.1 (2026-09-06) | Apache-2.0 OR MIT | 1.87 | 없음 | LLVM source-based coverage |
| insta | 1.48.0 (2026-06-11) | Apache-2.0 | 1.66.0 | 없음 | 스냅샷 테스트 |
| stateright | 0.31.0 (2025-07-27) | MIT | 미표기 | 없음 | "A library for model checking systems, with an emphasis on distributed systems." always/sometimes 속성 |

**제안 (테스트)**: 핵심은 (1) proptest 1.11.0으로 "임의 이벤트열 → 저널 → 재생 = 같은 상태/같은 출력" 속성, (2) 실제 프로세스를 SIGKILL하는 통합 시험, (3) 저널 디코더에 cargo-fuzz, (4) cargo-nextest + cargo-llvm-cov, (5) 주기적 cargo-mutants. loom은 직접 짠 lock-free 코드가 있을 때만, miri는 `unsafe`가 있을 때만 쓴다.
- 이유: 단일 작성자 코어는 동시성 탐색 대상이 거의 없으므로 loom/shuttle의 가치가 작다. 결정성은 속성 시험이 직접 겨냥한다.
- 주요 위험: Part 2 표의 "kill 시험: 아니오" 항목(fsync, 디렉터리 fsync, 장치 캐시, fsync 오류)은 이 도구들 어느 것으로도 검증되지 않는다. fsync 오류 경로는 I/O 계층을 trait로 추상화해 가짜 구현으로 오류를 주입하거나 device-mapper 오류 타깃을 써야 하고, 전원 장애는 실기 시험뿐이다. 접근한 범위에서 이를 대신해 주는 게시된 Rust crate는 찾지 못했다.

### 3.5 공급망

| 도구 | 최신 안정판 (날짜) | 라이선스 | MSRV | 설명 (crates.io) |
|---|---|---|---|---|
| cargo-deny | 0.20.2 (2026-07-09) | MIT OR Apache-2.0 | 1.88.0 | "Cargo plugin to help you manage large dependency graphs" |
| cargo-audit | 0.22.2 (2026-06-05) | Apache-2.0 OR MIT | 1.88 | "Audit Cargo.lock for crates with security vulnerabilities" |
| cargo-vet | 0.10.2 (2026-01-13) | Apache-2.0/MIT | 1.82 | "Supply-chain security for Rust" |
| cargo-auditable | 0.7.6 (2026-09-13) | MIT OR Apache-2.0 | 미표기 | "Make production Rust binaries auditable" |

네 도구 모두 RustSec 권고 없음. 각 도구의 세부 기능 문서는 이번에 읽지 않았다(설명 필드만 인용).

**제안 (공급망)**: cargo-deny 0.20.2 하나를 CI 게이트로(권고·라이선스·금지 crate·중복 버전), 배포 바이너리는 cargo-auditable 0.7.6으로 빌드. `Cargo.lock`을 커밋한다.
- 이유: 도구 수를 줄인다. 이번 조사에서 직접 의존 후보만 봐도 2026년 권고가 여럿 있었다(rustls, rtrb, rkyv, bytes, memmap2, aws-lc-sys, h2, rustls-webpki).
- 주요 위험: 권고 DB는 사후 탐지다. cargo-vet의 감사 기록 방식은 1인 프로젝트에 비용이 크다.

### 3.6 측정

| crate | 최신 안정판 (날짜) | 라이선스 | MSRV | RustSec | 메모 |
|---|---|---|---|---|---|
| hdrhistogram | 7.6.0 (2026-07-18) | MIT/Apache-2.0 | 1.88 | 없음 | coordinated omission 보정: `record_correct(value, interval)` — "Record a value in the histogram while correcting for coordinated omission." 사후 보정 `clone_correct`/`add_correct`. "The two methods are mutually exclusive, and only one of the two should be be used on a given data set" (원문 그대로) |
| criterion | 0.8.2 (2026-02-04) | Apache-2.0 OR MIT | 1.86 | 없음 | 저장소가 `criterion-rs/criterion.rs`로 표기됨 |
| divan | 0.1.21 (2025-04-10) | MIT OR Apache-2.0 | 1.80.0 | 없음 | 릴리스는 2025-04가 마지막(저장소 푸시 2026-07) |
| iai-callgrind | 0.16.1 (2025-07-30) | Apache-2.0 OR MIT | 1.74.1 | 없음 | GitHub `iai-callgrind/iai-callgrind`가 `gungraun/gungraun`으로 리다이렉트됨. crates.io에 `gungraun` 0.20.0 (2026-09-26, MSRV 1.88.0)이 있음. 개명 공지문 자체는 읽지 않음 |

**제안 (측정)**: 종단 지연은 hdrhistogram 7.6.0에 `record_correct`로 기록(부하 발생기가 고정 간격일 때), 마이크로벤치는 criterion 0.8.2.
- 이유: LMAX 논문의 지연 표와 같은 종류의 분포(평균, 99%, 99.99%, 최대)를 얻으려면 히스토그램이 필요하다.
- 주요 위험: `record_correct`와 `clone_correct`를 같은 데이터에 함께 쓰면 이중 보정이 된다(문서 경고). fsync가 포함된 경로는 마이크로벤치 분산이 크다.

### 3.7 어댑터용 네트워킹 / 암호

| crate | 최신 안정판 (날짜) | 라이선스 | MSRV | RustSec | 메모 |
|---|---|---|---|---|---|
| reqwest | 0.13.5 (2026-09-08) | MIT OR Apache-2.0 | 1.85.0 | 없음 | 0.13.5 features: `default-tls = ["rustls"]`, `rustls`가 `__rustls-aws-lc-rs`를 켬 → 기본 TLS는 rustls + aws-lc-rs |
| hyper | 1.11.1 (2026-08-28) | MIT | 1.63 | 과거 7건(전부 0.14 이하에서 패치) | — |
| tokio-tungstenite | 0.30.0 (2026-07-11) | MIT | 1.85 | 없음 (tungstenite: 2023-0065, ≥0.20.1 패치) | features: `rustls-tls-native-roots`, `rustls-tls-webpki-roots`, `native-tls` 등 |
| fastwebsockets | 0.10.0 (2025-01-14) | Apache-2.0 | 미표기 | 없음 | "a minimal, fast WebSocket server implementation". 0.8.1이 2026-01-01에 별도 게시됨(백포트로 보임) |
| rustls | 0.23.45 (2026-09-14) | Apache-2.0 OR ISC OR MIT | 1.71 | **RUSTSEC-2026-0285** (2026-09-14, "TLS 1.3 handshake messages incorrectly accepted across encryption level boundaries", ≥0.23.45 패치) 외 2건 | "by default it uses aws-lc-rs for implementing the cryptography in TLS". ring은 feature로 선택 |
| aws-lc-rs | 1.18.1 (2026-09-01) | ISC AND (Apache-2.0 OR ISC) | 1.71.0 | aws-lc-sys에 2026년 5건(0044–0048, ≥0.39.0 패치). 1.18.1은 `aws-lc-sys ^0.45.0` 요구 | C/어셈블리 빌드 의존 |
| ring | 0.17.14 (2025-03-11) | Apache-2.0 AND ISC | 1.66.0 | RUSTSEC-2025-0007 "unmaintained"(2025-02-20, **2025-02-22 철회**); 2025-0010 "<0.17 unmaintained"; 2025-0009(≥0.17.12 패치) | 릴리스는 2025-03이 마지막(저장소 푸시 2026-07) |
| k256 | 0.14.0 (2026-07-08) | Apache-2.0 OR MIT | 1.85 | 없음 | 순수 Rust secp256k1, ECDSA+복구. "This crate has been audited by NCC Group" … "USE AT YOUR OWN RISK!" |
| secp256k1 | 0.33.1 (2026-08-29) | CC0-1.0 | 1.63.0 | 1건(2022-0070, 패치됨) | "Rust bindings for Pieter Wuille's secp256k1 library" (C) |
| alloy | 2.5.0 (2026-09-23) | MIT OR Apache-2.0 | 1.94.1 | 없음 | 메타 crate |
| alloy-signer / alloy-signer-local | 2.5.0 (2026-09-23) | MIT OR Apache-2.0 | 1.94.1 | 없음 | `Signer::sign_hash`, `sign_typed_data<T: SolStruct>(…, domain: &Eip712Domain)` |
| alloy-sol-types / alloy-primitives | 1.7.3 (2026-09-10) | MIT OR Apache-2.0 | 1.85 | 없음 | `SolStruct::eip712_signing_hash(&self, domain: &Eip712Domain) -> B256` |
| sha3 | 0.12.0 (2026-05-15) | MIT OR Apache-2.0 | 1.85 | 없음 | RustCrypto |
| tiny-keccak | 2.0.2 (**2020-04-01**) | CC0-1.0 | 미표기 | 없음 | 릴리스가 6년 넘게 없음 |
| secrecy | 0.10.3 (2024-10-09) | Apache-2.0 OR MIT | 1.60 | 없음 | "Ensure secrets are wiped from memory on drop securely (using the zeroize crate)". 한계: "does not provide more advanced memory protection mechanisms e.g. ones based on mlock(2)/mprotect(2)" |
| zeroize | 1.9.0 (2026-06-12) | Apache-2.0 OR MIT | 1.85 | 없음 | "This crate guarantees the zeroing operation can't be "optimized away" by the compiler". 한계: "stack spilling and other optimizations may leave temporary copies of data from the heap on the stack" |
| (참고) hyperliquid_rust_sdk | 0.6.0 (2025-05-16) | MIT | 미표기 | 없음 | master는 `alloy = "1.0"`, `rmp-serde = "1.0"` 사용. 저장소 최근 푸시 2025-10-21 |

**제안 (네트워킹/암호)**: reqwest 0.13.5(기본 rustls/aws-lc-rs) + tokio-tungstenite 0.30.0(rustls feature) + alloy-primitives·alloy-sol-types 1.7.3·alloy-signer-local 2.5.0(EIP-712 서명) + rmp-serde(액션 해시) + secrecy/zeroize(키 보관). rustls는 **≥0.23.45**로 하한을 건다.
- 이유: Hyperliquid 공식 SDK가 alloy와 rmp-serde를 쓰므로 서명 바이트를 SDK와 대조하기 쉽다. TLS 공급자는 reqwest·rustls 기본값(aws-lc-rs)을 따르면 설정이 가장 적다.
- 주요 위험: alloy는 MSRV가 1.94.1로 툴체인(1.96)에 바짝 붙어 있고 의존 트리가 크다. 메타 crate `alloy` 대신 필요한 하위 crate만 쓴다. aws-lc-rs는 C 빌드가 필요하며 2026년에 aws-lc-sys 권고가 5건 있었다. secrecy/zeroize는 스왑·코어 덤프·스택 사본을 막지 못한다. `Signer` 트레이트는 async이므로 서명은 코어 밖(어댑터)에서 수행한다. 더 작은 대안은 k256 0.14.0 + sha3 0.12.0으로 EIP-712를 직접 구현하는 것이나 구현 오류 위험이 커진다. tiny-keccak은 직접 의존으로 들이지 않는다.

### 3.8 대시보드 HTTP / 메트릭 / 로깅

| crate | 최신 안정판 (날짜) | 라이선스 | MSRV | RustSec | 메모 |
|---|---|---|---|---|---|
| axum | 0.8.9 (2026-04-14) | MIT | 1.80 | 없음 | — |
| tower-http | 0.7.1 (2026-08-31) | MIT | 1.65 | 과거 2건(Windows 경로, 패치됨·1건 철회) | — |
| metrics | 0.24.6 (2026-05-13) | MIT | 1.71.1 | 없음 | "A lightweight metrics facade." (`metrics-exporter-prometheus` 0.18.3, 2026-04-30) |
| prometheus-client | 0.25.1 (2026-09-01) | Apache-2.0 OR MIT | 미표기 | 없음 | "Open Metrics client library allowing users to natively instrument applications." 저장소 `prometheus/client_rust` |
| tracing | 0.1.44 (2025-12-18) | MIT | 1.65.0 | 1건(2023-0078, ≥0.1.40 패치). tracing-subscriber: 2025-0055(≥0.3.20) | — |

이 절의 crate는 버전·권고만 확인했고 문서의 보장 문구는 읽지 않았다.

**제안**: axum 0.8.9 + tower-http 0.7.1(이미 tokio/hyper 계열) + prometheus-client 0.25.1 + tracing 0.1.44.
- 이유: reqwest와 hyper/tower 스택을 공유해 추가 의존이 적다.
- 주요 위험: 대시보드 핸들러가 코어 상태를 직접 읽으면 단일 작성자 원칙이 깨진다 — 코어가 내보낸 출력 이벤트로 만든 읽기 전용 사본만 본다. 코어 스레드 안에서 로그 포맷팅/할당을 하면 지터가 생긴다.

### 3.9 고정소수점 / 십진수

| crate | 최신 안정판 (날짜) | 라이선스 | MSRV | RustSec | 메모 |
|---|---|---|---|---|---|
| rust_decimal | 1.43.0 (2026-09-02) | MIT | 1.67.1 | 없음 | "The binary representation consists of a 96 bit integer number, a scaling factor used to specify the decimal fraction and a 1 bit sign." serde 표현은 feature(`serde-str`, `serde-float`, `serde-with-*`)에 따라 달라짐. `maths`의 `ln`/`log10`은 잘못된 입력에 panic |
| fixed | 1.31.0 (2026-03-20) | MIT/Apache-2.0 | 1.93 | 없음 | **이진** 고정소수점(`FixedI64<Frac>` 등). 소수 자릿수가 이진 분수 비트 |
| bigdecimal | 0.4.11 (2026-09-28) | MIT/Apache-2.0 | 미표기 | 없음 | "BigDecimal allows storing any real number to arbitrary precision". `BigInt` 기반(힙) |

**제안**: 코어 내부는 **직접 만든 `i64` newtype**(가격 틱, 수량 스텝 등 단위별 타입, `checked_*` 연산만 노출). rust_decimal 1.43.0은 거래소가 주는 십진 문자열을 파싱·검증하는 경계에서만 쓴다.
- 이유: 저널에 들어가는 값이 정수이면 직렬화가 플랫폼·버전과 무관하게 결정적이고 코어에 의존성이 없다. `fixed`는 이진 분수라 0.1 같은 십진 틱을 정확히 표현하지 못한다(문서의 표현 방식에서의 추론). bigdecimal은 힙 할당이 따른다.
- 주요 위험: 종목별 스케일 관리와 오버플로를 직접 책임져야 한다 — 스케일 혼동은 타입으로 막고 변환 함수는 속성 시험으로 검증한다. rust_decimal의 serde 표현은 feature에 좌우되므로 저널 형식에 그대로 넣지 않는다.

### 3.10 OS 인터페이스

| crate | 최신 안정판 (날짜) | 라이선스 | MSRV | RustSec | 메모 |
|---|---|---|---|---|---|
| rustix | 1.1.5 (2026-09-16) | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 1.65 | 없음 | `rustix::fs`: `fallocate(fd, mode, offset, len)`, `fdatasync`, `fsync`, `flock`, `fcntl_lock`, `ftruncate`, `renameat`, `syncfs` |
| nix | 0.31.3 (2026-05-11) | MIT | 1.69 | 1건(2021-0119, 패치됨) | libc 래퍼 |
| libc | 0.2.189 (2026-07-21) | MIT OR Apache-2.0 | 1.65 | 없음 | 1.0.0-alpha.5가 2026-10-02 게시됨(프리릴리스) |
| fs2 | 0.4.3 (**2018-01-06**) | MIT/Apache-2.0 | 미표기 | 없음 | 릴리스가 8년 넘게 없음 |
| fs4 | 1.1.0 (2026-04-28) | MIT OR Apache-2.0 | 1.75.0 | 없음 | "This is a fork of the fs2-rs crate", libc 대신 rustix |
| fd-lock | 4.0.4 (2025-03-10) | MIT OR Apache-2.0 | 미표기 | 없음 | ""advisory locks" are locks which programs must opt-in to adhere to" |
| core_affinity | 0.8.3 (2025-03-01) | MIT/Apache-2.0 | 미표기 | 없음 | `set_for_current(id)` |
| memmap2 | 0.9.11 (2026-06-22) | MIT OR Apache-2.0 | 1.65 | RUSTSEC-2026-0186 (2026-06-20, unsound "Unchecked pointer offset", ≥0.9.11 패치) | "All file-backed memory map constructors are marked unsafe because of the potential for Undefined Behavior (UB) using the map if the underlying file is subsequently modified" |
| (표준 라이브러리) `File::lock` 계열 | Rust 1.89.0부터 stable | — | — | — | "This function currently corresponds to the flock function on Unix with the LOCK_EX flag" / "This lock may be advisory or mandatory." / 락은 파일(및 복제된 fd)이 닫히면 해제 |

advisory 락 의미(B7): open file description에 묶이고, 같은 파일을 따로 `open`한 fd끼리는 독립이며, 모든 복제 fd가 닫히면 풀린다. 협조하지 않는 프로세스의 쓰기를 막지 못한다.

**제안 (OS)**: 파일 락은 표준 라이브러리 `File::try_lock`(1.89+, 현재 1.96)으로, fdatasync는 `File::sync_data`로 해결하고, **`fallocate`가 필요할 때만 rustix 1.1.5**를 추가한다. fs2·fd-lock·memmap2는 들이지 않는다. CPU 고정은 측정으로 필요가 확인되면 core_affinity 0.8.3 또는 rustix의 스레드 API.
- 이유: 표준 라이브러리로 되는 일을 crate로 하지 않는다. fs2는 사실상 방치 상태다. mmap 저널은 문서가 직접 UB 가능성을 경고하고, I/O 오류가 SIGBUS로 나타나 Part 2 (e)의 "오류 시 중단 후 재구성" 흐름에 맞지 않는다(뒤 절반은 추론).
- 주요 위험: `sync_data`→`fdatasync` 대응은 구현 사실이지 문서 보장이 아니다(B11). 디렉터리 fsync를 `File::open(dir)?.sync_all()`로 할 수 있다는 것은 std 문서에 명시돼 있지 않아 이번 조사에서 **확인 불가** — 실제로 호출해 확인하거나 rustix의 `fsync`를 디렉터리 fd에 쓴다. `File::lock` 문서는 "advisory or mandatory"와 "may change in the future"를 명시한다.

### 3.11 유지보수·권고 요주의 목록

| crate | 상태 | 근거 |
|---|---|---|
| bincode | **미유지보수. 3.0.0은 컴파일 오류만 내는 묘비 릴리스** | RUSTSEC-2025-0141; crate 파일 직접 확인 |
| fs2 | 최종 릴리스 2018-01-06 | crates.io (권고는 없음) |
| tiny-keccak | 최종 릴리스 2020-04-01 | crates.io (권고는 없음) |
| ring | 최종 릴리스 2025-03-11. "unmaintained" 권고는 이틀 만에 철회 | RUSTSEC-2025-0007(withdrawn 2025-02-22), 2025-0010 |
| loom | 최종 릴리스 2024-04-23 | crates.io |
| crc32c | 최종 릴리스 2024-06-09 | crates.io |
| secrecy | 최종 릴리스 2024-10-09 | crates.io |
| kanal / divan / stateright / madsim / iai-callgrind | 최종 릴리스 2025년(각 03-23 / 04-10 / 07-27 / 10-11 / 07-30) | crates.io. iai-callgrind는 gungraun으로 이어진 것으로 보임 |
| rkyv | 2026년 권고 5건(조작된 아카이브로 UAF·OOB) | RUSTSEC-2026-0001, 0122, 0233, 0234, 0235 — ≥0.8.17 패치 |
| rustls | 2026-09-14 권고 | RUSTSEC-2026-0285 — ≥0.23.45 필요 |
| rtrb | 2026-08-04 권고 | RUSTSEC-2026-0274 — 0.3.5/0.4.0 패치 |
| memmap2 | 2026-06-20 unsound 권고 | RUSTSEC-2026-0186 — ≥0.9.11 패치 |
| bytes | 2026-02-03 권고 | RUSTSEC-2026-0007 — ≥1.11.1 패치 |
| aws-lc-sys | 2026-03 권고 5건 | RUSTSEC-2026-0044–0048 — ≥0.39.0 패치 |
| h2, rustls-webpki (전이 의존) | 2026년 권고 | RUSTSEC-2026-0258(h2 ≥0.4.16), 2026-0049/0098/0099/0104(rustls-webpki) |
| turmoil-fs | crates.io에 없음 | API "does not exist" |

---

## 한계

**접근하지 못했거나 읽지 않은 것**

- Fowler 기사의 그림 4장(구조도·활동도)은 텍스트로 추출되지 않아 읽지 못했다. 본문이 그림 내용을 서술하는 범위까지만 반영했다.
- QCon 발표 영상, LMAX 직원 블로그, Disruptor GitHub wiki 원문은 열지 않았다. LMAX가 시간·재생 중 출력·저널 fsync를 어떻게 다뤘는지는 그 자료에 있을 수 있으나 이 문서의 "확인 불가"는 A1·A2·A4 범위에 한정된다.
- Samsung 980의 PLP·FLUSH 동작에 관한 제조사 자료와 제3자 실측은 가져오지 않았다. "PLP 없음"은 소유자 전제다.
- Hyperliquid 공식 API 문서는 가져오지 않았다. msgpack 해싱 방식은 공식 Rust SDK 소스 한 파일로만 확인했다.
- §3.5(공급망)과 §3.8(대시보드/메트릭)의 crate는 버전·라이선스·권고만 확인했고 문서의 보장 문구는 읽지 않았다.
- 각 crate의 문서는 최상위 페이지와 표에 적은 항목만 읽었다. 전체 API를 검토한 것이 아니다.
- rkyv의 버전 간 형식 안정성, miri의 fsync/fallocate/flock 지원, std에서 디렉터리 `sync_all` 가능 여부, EINTR 뒤 fsync 재호출의 안전성, ext4에서 unwritten extent 전환이 fdatasync에 포함되는지는 접근한 문서에 문장이 없었다.

**2차 자료이거나 오래된 것**

- LWN 기사 3건(B21–B23)은 2차 자료다. B23은 2011년 글이다.
- PostgreSQL wiki(B12)는 2023-07 최종 편집이며 Linux ≥4.16까지만 서술한다. 커널 7.0에서 달라진 점이 있는지는 확인하지 못했다.
- Rebello(B13)는 커널 5.2.11, Pillai(B14)는 2014년 당시 커널에서의 실측이다. 호스트는 7.0이다. 두 논문의 ext4 결과가 현재도 성립하는지는 재검증 자료가 없다.
- Pillai Table 1은 표가 그림으로 들어 있어 렌더링한 이미지를 눈으로 읽었다. 본문 문장과 맞춰 보았으나 열 판독 오류 가능성이 남는다.
- docs.kernel.org의 ext4·블록 문서는 "7.3.0-rc5" 표기였다. 호스트 커널(7.0)의 문서와 차이가 있을 수 있다.
- Disruptor 논문 HTML판은 2011 원본에 표 하나(현대 하드웨어)를 추가한 판이다. 2011년 수치는 원본 PDF와 대조했다.
- Rust std의 syscall 대응(B11)은 소스에서 읽은 구현 사실이며 문서화된 안정 보장이 아니다.

**이 문서 자체의 성격**

- "제안"·"추론"으로 표시한 내용은 조사자의 판단이며 출처의 진술이 아니다. 특히 §1.8의 "외부 전송만 게이트"(나)안과 재생 시 출력 억제·멱등 키·대사는 LMAX 원문에 근거가 없다.
- 성능 수치는 전부 LMAX의 Java 측정값이다. 이 OMS에서 측정한 값은 하나도 없다.
- crate 버전은 2026-10-02의 스냅샷이다.
