//! C# SkeletonJsonWriter 결과와 바이트 단위로 같은지(작은 고정 표본 하나: 42/coin-pro).
//! 표본 전체 비교는 examples/cs_parity.rs + tools/cs_oracle 로 한다.

mod support {
    pub mod cs_model;
}

#[test]
fn coin_pro_matches_csharp() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/coin-pro");
    let model = support::cs_model::model(&serde_json::from_str(&std::fs::read_to_string(format!("{dir}/0.model.json")).unwrap()).unwrap());
    for ver in ["4.0.64", "4.1.24", "4.2.43", "4.3.26"] {
        for (ne, tag) in [(true, "ne"), (false, "es")] {
            let want = std::fs::read_to_string(format!("{dir}/0.{ver}.{tag}.json")).unwrap();
            let got = spine_json::write_json(&model, ver, ne).unwrap();
            assert!(got == want, "{ver} {tag} 불일치");
        }
    }
}

#[test]
fn rejects_bad_version() {
    let m = spine_model::SkeletonModel::default();
    assert!(spine_json::write_json(&m, "abc", true).is_err());
}
