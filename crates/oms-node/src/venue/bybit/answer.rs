//! The one table of order and cancel answers (ADR-028 §7d, C-GW G-2,
//! G-13): placed or confirmed, rejected, or unknown.
//!
//! A documented non-zero retCode is an explicit refusal (`Rejected`) unless
//! `UNKNOWN_CODES` lists it: those say the server failed, the request is a
//! duplicate of one that may exist, or it is still being processed. Every
//! other answer is `Unknown`: an undocumented code, an HTTP status other
//! than 2xx, an unreadable body. The codes are from the Bybit error page
//! (B/v5/error, UTA and WS OE sections, read 2026-11-21).

use serde_json::Value;

/// Documented codes that do not prove the request was refused.
pub const UNKNOWN_CODES: [i64; 12] = [
    10000, 10014, 10016, 10019, 10429, 429, 20006, 40004, 110030, 110072, 110079, 3400214,
];

/// Documented codes that refuse the request. Sorted.
pub const REJECTED_CODES: &[i64] = &[
    -2015, -1, 10001, 10002, 10003, 10004, 10005, 10006, 10007, 10008, 10009, 10010, 10017, 10018,
    10024, 10027, 10028, 10029, 10404, 20003, 30035, 30133, 30134, 30135, 30136, 30208, 30209,
    30228, 30256, 33004, 34040, 100028, 110001, 110003, 110004, 110005, 110006, 110007, 110008,
    110009, 110010, 110011, 110012, 110013, 110014, 110015, 110016, 110017, 110018, 110019, 110020,
    110021, 110022, 110023, 110024, 110025, 110026, 110027, 110028, 110029, 110031, 110032, 110033,
    110034, 110035, 110036, 110037, 110038, 110039, 110040, 110041, 110042, 110043, 110044, 110045,
    110046, 110047, 110048, 110049, 110050, 110051, 110052, 110053, 110054, 110055, 110056, 110057,
    110058, 110059, 110060, 110061, 110062, 110063, 110064, 110065, 110066, 110067, 110068, 110069,
    110070, 110071, 110073, 110074, 110075, 110076, 110077, 110078, 110080, 110082, 110083, 110085,
    110086, 110087, 110088, 110089, 110090, 110092, 110093, 110094, 110095, 110096, 110097, 110098,
    110099, 110100, 110101, 110102, 110103, 110104, 110105, 110106, 110107, 110108, 110109, 110110,
    110111, 110112, 110113, 110114, 110115, 110116, 110117, 110118, 110119, 110120, 110121, 110123,
    110124, 110125, 110131, 110132, 110134, 110135, 110136, 110137, 170346, 170360, 181017, 182011,
    182012, 182100, 182101, 182102, 182103, 182108, 182110, 182111, 182112, 500010, 500011,
    3100101, 3100181, 3100197, 3100326, 3200306, 3200316, 3200317, 3200320, 3200403, 3200419,
    3200425, 3200641, 3400052, 3400053, 3400054, 3400071, 3400139, 3400208, 3400209, 3400210,
    3400211, 3400212, 3400213, 3401010,
];

/// What an order or cancel answer says.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// retCode 0: the `result` object.
    Accepted(Value),
    /// The venue refused the request: it was not processed.
    Rejected { code: i64, msg: String },
    /// The request may or may not have been processed.
    Unknown(String),
}

/// Classifies one answer by HTTP status and body (REST) or by the body
/// alone (WS Trade, status 200).
pub fn classify(status: u16, body: &str) -> Answer {
    let short: String = body.chars().take(200).collect();
    if !(200..=299).contains(&status) {
        return Answer::Unknown(format!("HTTP {status}: {short}"));
    }
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return Answer::Unknown(format!("unreadable answer: {short}"));
    };
    let Some(code) = v.get("retCode").and_then(Value::as_i64) else {
        return Answer::Unknown(format!("no retCode: {short}"));
    };
    let msg = v
        .get("retMsg")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if code == 0 {
        // The WS Trade answer carries the object as `data`, REST as `result`.
        let result = v
            .get("result")
            .or_else(|| v.get("data"))
            .cloned()
            .unwrap_or(Value::Null);
        return Answer::Accepted(result);
    }
    if REJECTED_CODES.binary_search(&code).is_ok() {
        return Answer::Rejected { code, msg };
    }
    Answer::Unknown(format!("retCode {code}: {msg}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every non-zero code on the error page (UTA and WS OE), as of
    /// 2026-11-21.
    const DOCUMENTED: &[i64] = &[
        -2015, -1, 429, 10000, 10001, 10002, 10003, 10004, 10005, 10006, 10007, 10008, 10009,
        10010, 10014, 10016, 10017, 10018, 10019, 10024, 10027, 10028, 10029, 10404, 10429, 20003,
        20006, 30035, 30133, 30134, 30135, 30136, 30208, 30209, 30228, 30256, 33004, 34040, 40004,
        100028, 110001, 110003, 110004, 110005, 110006, 110007, 110008, 110009, 110010, 110011,
        110012, 110013, 110014, 110015, 110016, 110017, 110018, 110019, 110020, 110021, 110022,
        110023, 110024, 110025, 110026, 110027, 110028, 110029, 110030, 110031, 110032, 110033,
        110034, 110035, 110036, 110037, 110038, 110039, 110040, 110041, 110042, 110043, 110044,
        110045, 110046, 110047, 110048, 110049, 110050, 110051, 110052, 110053, 110054, 110055,
        110056, 110057, 110058, 110059, 110060, 110061, 110062, 110063, 110064, 110065, 110066,
        110067, 110068, 110069, 110070, 110071, 110072, 110073, 110074, 110075, 110076, 110077,
        110078, 110079, 110080, 110082, 110083, 110085, 110086, 110087, 110088, 110089, 110090,
        110092, 110093, 110094, 110095, 110096, 110097, 110098, 110099, 110100, 110101, 110102,
        110103, 110104, 110105, 110106, 110107, 110108, 110109, 110110, 110111, 110112, 110113,
        110114, 110115, 110116, 110117, 110118, 110119, 110120, 110121, 110123, 110124, 110125,
        110131, 110132, 110134, 110135, 110136, 110137, 170346, 170360, 181017, 182011, 182012,
        182100, 182101, 182102, 182103, 182108, 182110, 182111, 182112, 500010, 500011, 3100101,
        3100181, 3100197, 3100326, 3200306, 3200316, 3200317, 3200320, 3200403, 3200419, 3200425,
        3200641, 3400052, 3400053, 3400054, 3400071, 3400139, 3400208, 3400209, 3400210, 3400211,
        3400212, 3400213, 3400214, 3401010,
    ];

    #[test]
    fn every_documented_code_has_exactly_one_class() {
        assert!(REJECTED_CODES.windows(2).all(|w| w[0] < w[1]), "not sorted");
        for c in DOCUMENTED {
            let rejected = REJECTED_CODES.contains(c);
            let unknown = UNKNOWN_CODES.contains(c);
            assert!(
                rejected != unknown,
                "code {c}: rejected {rejected}, unknown {unknown}"
            );
            let body = format!(r#"{{"retCode":{c},"retMsg":"m"}}"#);
            let class = classify(200, &body);
            if rejected {
                assert_eq!(
                    class,
                    Answer::Rejected {
                        code: *c,
                        msg: "m".into()
                    }
                );
            } else {
                assert!(matches!(class, Answer::Unknown(_)), "{c}: {class:?}");
            }
        }
        // Nothing in the tables that the page does not list.
        for c in REJECTED_CODES.iter().chain(&UNKNOWN_CODES) {
            assert!(DOCUMENTED.contains(c), "code {c} is not documented");
        }
    }

    #[test]
    fn anything_but_a_documented_refusal_is_unknown() {
        let cases = [
            (200, r#"{"retCode":999999,"retMsg":"new"}"#),
            (
                200,
                r#"{"retCode":110072,"retMsg":"OrderLinkedID is duplicate"}"#,
            ),
            (200, r#"{"retCode":10016,"retMsg":"Server error."}"#),
            (200, r#"{"retMsg":"no code"}"#),
            (200, "<html>"),
            (200, ""),
            (401, r#"{"retCode":10003,"retMsg":"bad key"}"#),
            (403, "access too frequent"),
            (429, r#"{"retCode":10006}"#),
            (500, r#"{"retCode":0,"result":{}}"#),
            (502, ""),
        ];
        for (status, body) in cases {
            assert!(
                matches!(classify(status, body), Answer::Unknown(_)),
                "{status} {body}"
            );
        }
    }

    #[test]
    fn code_zero_is_accepted_with_its_object_from_rest_or_ws() {
        let rest = r#"{"retCode":0,"retMsg":"OK","result":{"orderId":"1","orderLinkId":"a"}}"#;
        let ws = r#"{"reqId":"r","retCode":0,"retMsg":"OK","op":"order.create","data":{"orderId":"2","orderLinkId":"b"}}"#;
        assert_eq!(
            classify(200, rest),
            Answer::Accepted(serde_json::json!({"orderId":"1","orderLinkId":"a"}))
        );
        assert_eq!(
            classify(200, ws),
            Answer::Accepted(serde_json::json!({"orderId":"2","orderLinkId":"b"}))
        );
        assert_eq!(
            classify(
                200,
                r#"{"retCode":110007,"retMsg":"Available balance is insufficient"}"#
            ),
            Answer::Rejected {
                code: 110007,
                msg: "Available balance is insufficient".into()
            }
        );
    }
}
