// SPDX-License-Identifier: MPL-2.0
use std::process::Command;
#[test]
fn configuration_is_absolute_and_metadata_is_consistent() {
    let output = Command::new(env!("CARGO_BIN_EXE_ru-dbviewer"))
        .args(["--print-config", "--plugin-id", "database"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["plugins"][0]["id"], "database");
    assert!(std::path::Path::new(v["plugins"][0]["executable"].as_str().unwrap()).is_absolute());
    assert_eq!(v["plugins"][0]["runyte"], ru_dbviewer::HOST_RANGE);
}
#[test]
fn invalid_configuration_arguments_fail() {
    for args in [
        vec!["--print-config", "--plugin-id", "Bad ID"],
        vec!["--print-config", "--unexpected"],
        vec!["--unknown"],
    ] {
        assert!(
            !Command::new(env!("CARGO_BIN_EXE_ru-dbviewer"))
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}
