// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Reproducible GitHub Pages archives and credential-free handoff.
use std::fs;
use std::path::Path;

use flate2::Compression;
use flate2::GzBuilder;
use serde_json::json;

use crate::identity::artifact_digest;
use crate::identity::files;
use crate::pages_error::PagesError;
use crate::report::Run;
use crate::report::write_json;
pub(crate) fn create(run: &mut Run, site: &Path, deploy: bool) -> Result<(), PagesError> {
    let path = run.work.join("pages.tar.gz");
    let file = tempfile::NamedTempFile::new_in(&run.work)?;
    let encoder = GzBuilder::new().mtime(0).write(file, Compression::default());
    let mut tar = tar::Builder::new(encoder);
    for relative in files(site)? {
        crate::api::ensure_active()?;
        let bytes = fs::read(site.join(&relative))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        tar.append_data(&mut header, relative, bytes.as_slice())?;
    }
    crate::api::ensure_active()?;
    run.fresh()?;
    let file = tar.into_inner()?.finish()?;
    file.persist(&path).map_err(PagesError::persistence)?;
    let metadata = run.work.join("artifact.json");
    write_json(
        &metadata,
        &json!({"schema":1,"archive":"pages.tar.gz","sha256":artifact_digest(&path)?,"size":fs::metadata(&path)?.len(),"site_manifest_sha256":artifact_digest(&site.join("site-manifest.json"))?}),
    )?;
    run.add_artifact("pages-archive", &path)?;
    run.add_artifact("artifact-metadata", &metadata)?;
    if deploy {
        let handoff = run.work.join("deploy-handoff.json");
        write_json(
            &handoff,
            &json!({"schema":1,"site_directory":site,"archive":path,"artifact_metadata":metadata,"upload_action":"actions/upload-pages-artifact","deploy_action":"actions/deploy-pages","requires_authenticated_workflow":true,"quality_gates_passed":true}),
        )?;
        run.add_artifact("deploy-handoff", &handoff)?;
        println!("{}", crate::report::read_json(&handoff)?);
    }
    Ok(())
}
