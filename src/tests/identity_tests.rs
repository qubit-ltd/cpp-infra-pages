// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::identity::files;
use crate::pages_error::PagesError;
#[test]
fn test_missing_artifact_root_preserves_traversal_failure() {
    let root = tempfile::tempdir().expect("artifact root");
    let missing = root.path().join("missing-artifact-directory");
    let error = files(&missing).expect_err("missing directory must not become empty successful input");
    assert!(matches!(&error, PagesError::Io(_)));
    assert!(error.to_string().contains("missing-artifact-directory"));
}
