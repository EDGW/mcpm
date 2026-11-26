//! Download regressions for HTTP progress/cleanup, mapping tables, and built-in sources.

use std::{
    cell::RefCell,
    io::{Read, Write},
    net::TcpListener,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use super::*;
use catalog::{
    FabricInstallerEntry, FabricLoaderEntry, ForgeVersionEntry, NeoForgeVersionEntry,
};
use sources::{BmclapiCatalog, MappedSource, OfficialCatalog, OfficialSource, UrlMapping, bmclapi};

#[test]
fn fallback_resets_bytes_and_completion_requires_a_verified_published_file() {
    for (second_body, second_length) in [
        (b"agent".as_slice(), false),
        (b"agent".as_slice(), true),
        (b"broken".as_slice(), false),
    ] {
        let fixture = Fixture::new(vec![(b"broken", true), (second_body, second_length)]);
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("agent.jar");
        // Quiet APIs keep their Send futures even though observers may opt into local state.
        fn assert_send(_: impl Send) {}
        assert_send(fixture.downloader.download_to(&request(), &target));
        assert_send(authlib_injector::latest(&fixture.downloader));
        assert_send(mojang::resolve_server(
            &fixture.downloader,
            "latest",
            &|_| Ok(()),
        ));
        let events = RefCell::new(Vec::new());
        let result = runtime().block_on(fixture.downloader.download_to_with_progress(
            &request(),
            &target,
            &|event| {
                // Completion is emitted only after publication, not on the final byte.
                if matches!(event, Event::Completed { .. }) {
                    assert_eq!(std::fs::read(&target).unwrap(), b"agent");
                }
                events.borrow_mut().push(Recorded::from(event));
                Ok(())
            },
        ));
        let events = events.into_inner();
        let second = events
            .iter()
            .position(|event| *event == Recorded::Started("second".into()))
            .unwrap();
        assert_eq!(events[0], Recorded::Started("first".into()));
        assert_eq!(events[1], Recorded::Bytes(0, Some(6)));
        assert!(events[..second].contains(&Recorded::Bytes(6, Some(6))));
        assert!(events[..second].contains(&Recorded::Verifying));
        assert!(events[..second].contains(&Recorded::Failed));
        let total = second_length.then_some(second_body.len() as u64);
        assert_eq!(events[second + 1], Recorded::Bytes(0, total));
        assert!(events[second..].contains(&Recorded::Bytes(second_body.len() as u64, total)));
        if second_body == b"agent" {
            result.unwrap();
            assert_eq!(events.last(), Some(&Recorded::Completed));
        } else {
            assert!(result.unwrap_err().is::<crate::error::CombinedError>());
            assert!(!events.contains(&Recorded::Completed));
            assert!(!target.exists());
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        }
        fixture.thread.join().unwrap();
    }
}

#[test]
fn observer_failure_during_transfer_aborts_without_fallback_or_publication() {
    #[derive(Debug, thiserror::Error)]
    #[error("progress consumer failed")]
    struct ConsumerFailed;

    for fail_on_notification in [false, true] {
        let body = if fail_on_notification {
            b"broken".as_slice()
        } else {
            b"agent".as_slice()
        };
        let fixture = Fixture::new(vec![(body, true)]);
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("agent.jar");
        let events = RefCell::new(Vec::new());
        let error = runtime()
            .block_on(
                fixture
                    .downloader
                    .download_to_with_progress(&request(), &target, &|event| {
                        events.borrow_mut().push(Recorded::from(event));
                        if (fail_on_notification && matches!(event, Event::AttemptFailed { .. }))
                            || (!fail_on_notification
                                && matches!(event, Event::Bytes { received, .. } if received > 0))
                        {
                            return Err(ConsumerFailed.into());
                        }
                        Ok(())
                    }),
            )
            .unwrap_err();
        assert!(crate::error::contains::<ConsumerFailed>(error.as_ref()));
        if fail_on_notification {
            assert!(crate::error::contains::<ChecksumMismatch>(error.as_ref()));
        }
        assert!(
            !events
                .borrow()
                .contains(&Recorded::Started("second".into()))
        );
        assert!(!events.borrow().contains(&Recorded::Completed));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        fixture.thread.join().unwrap();
    }
}

#[test]
fn mapping_table_rejects_duplicates_and_sorts_longest_prefix_first() {
    let mapping = |original: &str, mirror: &str| {
        UrlMapping::new(original.parse().unwrap(), mirror.parse().unwrap()).unwrap()
    };

    let err = MappingTable::new(vec![
        mapping("https://example.invalid/maven/", "https://m1.invalid/maven/"),
        mapping("https://example.invalid/maven/", "https://m2.invalid/maven/"),
    ])
    .unwrap_err();
    assert!(err.to_string().contains("duplicate"));

    let table = MappingTable::new(vec![
        mapping("https://example.invalid/", "https://m.invalid/root/"),
        mapping("https://a.invalid/maven/", "https://m.invalid/a/"),
        mapping("https://b.invalid/maven/", "https://m.invalid/b/"),
        mapping(
            "https://example.invalid/maven/releases/",
            "https://m.invalid/releases/",
        ),
    ])
    .unwrap();
    let originals: Vec<&str> = table
        .entries()
        .iter()
        .map(|entry| entry.original().as_str())
        .collect();
    assert_eq!(
        originals,
        vec![
            "https://example.invalid/maven/releases/",
            "https://a.invalid/maven/",
            "https://b.invalid/maven/",
            "https://example.invalid/",
        ]
    );
}

#[test]
fn source_mapping_tables_and_bmclapi_creeperhost() {
    let official = OfficialSource.mapping_table().unwrap().unwrap();
    assert!(official.is_empty());
    assert!(official.entries().is_empty());

    let mapped = MappedSource::new(
        "mirror",
        vec![
            UrlMapping::new(
                "https://example.invalid/".parse().unwrap(),
                "https://primary.invalid/root/".parse().unwrap(),
            )
            .unwrap(),
            UrlMapping::new(
                "https://example.invalid/".parse().unwrap(),
                "https://secondary.invalid/root/".parse().unwrap(),
            )
            .unwrap(),
            UrlMapping::new(
                "https://example.invalid/maven/".parse().unwrap(),
                "https://primary.invalid/maven/".parse().unwrap(),
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let table = mapped.mapping_table().unwrap().unwrap();
    let pairs: Vec<(&str, &str)> = table
        .entries()
        .iter()
        .map(|entry| (entry.original().as_str(), entry.mirror().as_str()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            (
                "https://example.invalid/maven/",
                "https://primary.invalid/maven/",
            ),
            ("https://example.invalid/", "https://primary.invalid/root/"),
        ]
    );

    let empty_mapped = MappedSource::new("empty", vec![]).unwrap();
    assert!(empty_mapped.mapping_table().unwrap().is_none());

    let bmcl = bmclapi().unwrap();
    let translated = bmcl
        .translate(&"https://maven.creeperhost.net/org/example/mod.jar".parse().unwrap())
        .unwrap();
    assert_eq!(
        translated,
        vec![
            url::Url::parse("https://bmclapi2.bangbang93.com/maven/org/example/mod.jar").unwrap()
        ]
    );
    let bmcl_table = bmcl.mapping_table().unwrap().unwrap();
    assert!(bmcl_table.entries().iter().any(|entry| {
        entry.original().as_str() == "https://maven.creeperhost.net/"
            && entry.mirror().as_str() == "https://bmclapi2.bangbang93.com/maven/"
    }));
}

#[test]
fn official_catalog_decodes_forge_metadata_and_promotions_tolerantly() {
    let endpoint = OfficialCatalog::forge_versions_endpoint().unwrap();
    assert_eq!(
        endpoint.url.as_str(),
        "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml"
    );
    let xml = br#"
        <metadata>
          <versioning>
            <versions>
              <version>1.5.2-7.8.1.738</version>
              <version>1.7.10-10.13.4.1614-1.7.10</version>
              <version>1.7.10-10.13.2.1343-1.7.10ls</version>
              <version>1.20.4-49.2.0</version>
              <version>1.20.4-asdfjk</version>
              <version>1.16.5-rc1-36.2.0</version>
              <version>bad/path</version>
              <version>bad\path</version>
              <version>   </version>
            </versions>
          </versioning>
        </metadata>
    "#;
    let parsed = (endpoint.decode)(xml).unwrap();
    let raw = |coord: &str| ForgeVersionEntry {
        maven_coord: coord.into(),
        mc_version: None,
        loader_version: None,
    };
    assert_eq!(
        parsed,
        vec![
            raw("1.5.2-7.8.1.738"),
            raw("1.7.10-10.13.4.1614-1.7.10"),
            raw("1.7.10-10.13.2.1343-1.7.10ls"),
            raw("1.20.4-49.2.0"),
            raw("1.20.4-asdfjk"),
            raw("1.16.5-rc1-36.2.0"),
        ]
    );
    assert!((endpoint.decode)(b"<metadata></metadata>").is_err());

    let promos_endpoint = OfficialCatalog::forge_promotions_endpoint().unwrap();
    assert_eq!(
        promos_endpoint.url.as_str(),
        "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json"
    );
    let promos = (promos_endpoint.decode)(
        br#"{
            "homepage": "https://files.minecraftforge.net/",
            "promos": {
                "1.20.4-recommended": "49.2.0",
                "1.20.4-latest": "49.2.1",
                "1.7.10-recommended": "10.13.4.1614",
                "ignored-key": "1.0.0",
                "-latest": "0.0.1"
            }
        }"#,
    )
    .unwrap();
    assert_eq!(
        promos.recommended.get("1.20.4").map(String::as_str),
        Some("49.2.0")
    );
    assert_eq!(
        promos.latest.get("1.20.4").map(String::as_str),
        Some("49.2.1")
    );
    assert_eq!(promos.recommended.len(), 2);
    assert_eq!(promos.latest.len(), 1);
}

#[test]
fn bmclapi_catalog_overrides_forge_and_neoforge_endpoints() {
    let bmcl = bmclapi().unwrap();
    let catalog = bmcl.catalog().expect("bmclapi attaches BmclapiCatalog");
    assert!(catalog.minecraft_versions().unwrap().is_none());
    assert!(catalog.authlib_latest().unwrap().is_none());
    assert!(catalog.neoforge_versions("1.21.1").unwrap().is_some());
    assert!(catalog.fabric_loaders("1.21.1").unwrap().is_none());
    assert!(catalog.fabric_installers().unwrap().is_none());

    let versions_ep = BmclapiCatalog.forge_versions("1.7.10").unwrap().unwrap();
    assert_eq!(
        versions_ep.url.as_str(),
        "https://bmclapi2.bangbang93.com/forge/minecraft/1.7.10"
    );
    let versions = (versions_ep.decode)(
        br#"[
            {"mcversion": "1.7.10", "version": "10.13.4.1614", "branch": "1.7.10"},
            {"mcversion": "1.20.4", "version": "49.2.0"},
            {"mcversion": "1.20.4", "version": "49.1.0", "branch": ""},
            {"mcversion": "", "version": "1.0.0"}
        ]"#,
    )
    .unwrap();
    assert_eq!(
        versions,
        vec![
            ForgeVersionEntry {
                maven_coord: "1.7.10-10.13.4.1614-1.7.10".into(),
                mc_version: Some("1.7.10".into()),
                loader_version: Some("10.13.4.1614".into()),
            },
            ForgeVersionEntry {
                maven_coord: "1.20.4-49.2.0".into(),
                mc_version: Some("1.20.4".into()),
                loader_version: Some("49.2.0".into()),
            },
            ForgeVersionEntry {
                maven_coord: "1.20.4-49.1.0".into(),
                mc_version: Some("1.20.4".into()),
                loader_version: Some("49.1.0".into()),
            },
        ]
    );
    assert!(BmclapiCatalog.forge_versions("../1.20.4").is_err());

    let promos_ep = BmclapiCatalog.forge_promotions().unwrap().unwrap();
    assert_eq!(
        promos_ep.url.as_str(),
        "https://bmclapi2.bangbang93.com/forge/promos"
    );
    let promos = (promos_ep.decode)(
        br#"[
            {"name": "1.20.4-recommended", "build": {"version": "49.2.0"}},
            {"name": "1.20.4-latest", "build": {"version": "49.2.1"}},
            {"name": "1.21-recommended", "build": null},
            {"name": "latest", "build": {"version": "54.0.0"}},
            {"name": "recommended", "build": {"version": "54.0.0"}}
        ]"#,
    )
    .unwrap();
    assert_eq!(
        promos.recommended.get("1.20.4").map(String::as_str),
        Some("49.2.0")
    );
    assert_eq!(
        promos.latest.get("1.20.4").map(String::as_str),
        Some("49.2.1")
    );
    assert_eq!(promos.recommended.len(), 1);
    assert_eq!(promos.latest.len(), 1);

    // BMCLAPI NeoForge list endpoints (oldest-first upstream reversed to newest-first).
    let neo_261 = BmclapiCatalog.neoforge_versions("26.1").unwrap().unwrap();
    assert_eq!(
        neo_261.url.as_str(),
        "https://bmclapi2.bangbang93.com/neoforge/list/26.1"
    );
    let decoded_261 = (neo_261.decode)(
        br#"[
            {"mcversion": "26.1", "version": "26.1.0.1-beta", "rawVersion": "neoforge-26.1.0.1-beta"},
            {"mcversion": "26.1", "rawVersion": "neoforge-asdfjk"},
            {"mcversion": "26.1", "version": "26.1.0.19-beta", "rawVersion": "neoforge-26.1.0.19-beta"},
            {"mcversion": "", "version": "26.1.0.20-beta"},
            {"mcversion": "26.1", "version": "bad/path"}
        ]"#,
    )
    .unwrap();
    assert_eq!(
        decoded_261,
        vec![
            NeoForgeVersionEntry {
                mc_version: "26.1".into(),
                loader_version: "26.1.0.19-beta".into(),
                maven_coord: "26.1.0.19-beta".into(),
                stable: None,
            },
            NeoForgeVersionEntry {
                mc_version: "26.1".into(),
                loader_version: "asdfjk".into(),
                maven_coord: "asdfjk".into(),
                stable: None,
            },
            NeoForgeVersionEntry {
                mc_version: "26.1".into(),
                loader_version: "26.1.0.1-beta".into(),
                maven_coord: "26.1.0.1-beta".into(),
                stable: None,
            },
        ]
    );

    let neo_1201 = BmclapiCatalog.neoforge_versions("1.20.1").unwrap().unwrap();
    let decoded_1201 = (neo_1201.decode)(
        br#"[
            {"mcversion": "1.20.1", "version": "47.1.5", "rawVersion": "1.20.1-47.1.5"},
            {"mcversion": "1.20.1", "rawVersion": "1.20.1-forge-47.1.80"},
            {"mcversion": "1.20.1", "version": "1.20.1-47.1.105", "rawVersion": "1.20.1-forge-47.1.105"}
        ]"#,
    )
    .unwrap();
    assert_eq!(
        decoded_1201,
        vec![
            NeoForgeVersionEntry {
                mc_version: "1.20.1".into(),
                loader_version: "47.1.105".into(),
                maven_coord: "1.20.1-47.1.105".into(),
                stable: None,
            },
            NeoForgeVersionEntry {
                mc_version: "1.20.1".into(),
                loader_version: "47.1.80".into(),
                maven_coord: "1.20.1-47.1.80".into(),
                stable: None,
            },
            NeoForgeVersionEntry {
                mc_version: "1.20.1".into(),
                loader_version: "47.1.5".into(),
                maven_coord: "1.20.1-47.1.5".into(),
                stable: None,
            },
        ]
    );
    assert!(BmclapiCatalog.neoforge_versions("../26.1").is_err());
}

#[test]
fn official_catalog_decodes_neoforge_modrinth_manifest() {
    let ep = OfficialCatalog::neoforge_versions_endpoint().unwrap();
    assert_eq!(
        ep.url.as_str(),
        "https://launcher-meta.modrinth.com/neo/v0/manifest.json"
    );
    let entries = (ep.decode)(
        br#"{
            "gameVersions": [
                {
                    "id": "1.20.1",
                    "loaders": [
                        {"id": "47.1.106", "url": "not-even-a-url-must-be-ignored", "stable": false},
                        {"id": "1.20.1-47.1.105", "url": "https://ignored.invalid/1", "stable": true},
                        {"id": "bad/path", "url": "https://ignored.invalid/2", "stable": false}
                    ]
                },
                {
                    "id": "26.1",
                    "loaders": [
                        {"id": "26.1.0.19-beta", "url": "https://ignored.invalid/3", "stable": false},
                        {"id": "asdfjk", "stable": true}
                    ]
                },
                {
                    "id": "26.1.2",
                    "loaders": [
                        {"id": "26.1.2.130", "url": "https://ignored.invalid/4", "stable": false}
                    ]
                },
                {
                    "id": " ",
                    "loaders": [
                        {"id": "99.0.0", "stable": true}
                    ]
                }
            ]
        }"#,
    )
    .unwrap();
    assert_eq!(
        entries,
        vec![
            NeoForgeVersionEntry {
                mc_version: "1.20.1".into(),
                loader_version: "47.1.106".into(),
                maven_coord: "1.20.1-47.1.106".into(),
                stable: Some(false),
            },
            NeoForgeVersionEntry {
                mc_version: "1.20.1".into(),
                loader_version: "47.1.105".into(),
                maven_coord: "1.20.1-47.1.105".into(),
                stable: Some(true),
            },
            NeoForgeVersionEntry {
                mc_version: "26.1".into(),
                loader_version: "26.1.0.19-beta".into(),
                maven_coord: "26.1.0.19-beta".into(),
                stable: Some(false),
            },
            NeoForgeVersionEntry {
                mc_version: "26.1".into(),
                loader_version: "asdfjk".into(),
                maven_coord: "asdfjk".into(),
                stable: Some(true),
            },
            NeoForgeVersionEntry {
                mc_version: "26.1.2".into(),
                loader_version: "26.1.2.130".into(),
                maven_coord: "26.1.2.130".into(),
                stable: Some(false),
            },
        ]
    );
}

#[test]
fn official_catalog_decodes_fabric_loaders_and_installers() {
    let loaders_ep = OfficialCatalog.fabric_loaders("1.21.4").unwrap().unwrap();
    assert_eq!(
        loaders_ep.url.as_str(),
        "https://meta.fabricmc.net/v2/versions/loader/1.21.4"
    );
    let loaders = (loaders_ep.decode)(
        br#"[
            {"loader": {"version": "0.16.10", "stable": true}},
            {"loader": {"version": "0.16.9-beta.1", "stable": false}},
            {"loader": {"version": "   ", "stable": true}}
        ]"#,
    )
    .unwrap();
    assert_eq!(
        loaders,
        vec![
            FabricLoaderEntry {
                version: "0.16.10".into(),
                stable: true,
            },
            FabricLoaderEntry {
                version: "0.16.9-beta.1".into(),
                stable: false,
            },
        ]
    );
    assert!(OfficialCatalog.fabric_loaders("1.21/../../etc").is_err());

    let installers_ep = OfficialCatalog.fabric_installers().unwrap().unwrap();
    assert_eq!(
        installers_ep.url.as_str(),
        "https://meta.fabricmc.net/v2/versions/installer"
    );
    let installers = (installers_ep.decode)(
        br#"[
            {"version": "1.0.1", "maven": "net.fabricmc:fabric-installer:1.0.1", "stable": true},
            {"version": "1.0.0", "maven": "", "stable": false}
        ]"#,
    )
    .unwrap();
    assert_eq!(
        installers,
        vec![FabricInstallerEntry {
            version: "1.0.1".into(),
            maven_coord: "net.fabricmc:fabric-installer:1.0.1".into(),
            stable: true,
        }]
    );
}

#[test]
fn mojang_select_ignores_unrelated_empty_or_duplicate_entries() {
    let endpoint = OfficialCatalog.minecraft_versions().unwrap().unwrap();
    let list = (endpoint.decode)(
        br#"{
            "latest": {"release": "1.21.4", "snapshot": ""},
            "versions": [
                {"id": "", "url": "https://piston-meta.mojang.com/empty.json", "releaseTime": "2024-01-01T00:00:00+00:00"},
                {"id": "1.0", "url": "https://piston-meta.mojang.com/dup1.json", "releaseTime": "2011-11-18T00:00:00+00:00"},
                {"id": "1.0", "url": "https://piston-meta.mojang.com/dup2.json", "releaseTime": "2011-11-18T00:00:00+00:00"},
                {"id": "1.21.4", "url": "https://piston-meta.mojang.com/1.21.4.json", "sha1": "0123456789abcdef0123456789abcdef01234567", "releaseTime": "2024-12-03T10:12:57+00:00"}
            ]
        }"#,
    )
    .unwrap();

    let latest = mojang::select(list.clone(), "latest").unwrap();
    assert_eq!(latest.id, "1.21.4");
    assert_eq!(
        latest.url.as_str(),
        "https://piston-meta.mojang.com/1.21.4.json"
    );

    let exact = mojang::select(list.clone(), "1.21.4").unwrap();
    assert_eq!(exact.id, "1.21.4");

    assert!(mojang::select(list, "latest-snapshot").is_err());
}

#[test]
fn catalog_lag_falls_back_and_fixed_installer_download_never_requeries_catalog() {
    struct LaggingMirrorCatalog {
        forge_versions_url: url::Url,
    }

    impl Catalog for LaggingMirrorCatalog {
        fn forge_versions(
            &self,
            _mc: &str,
        ) -> anyhow::Result<Option<CatalogEndpoint<Vec<ForgeVersionEntry>>>> {
            Ok(Some(CatalogEndpoint {
                url: self.forge_versions_url.clone(),
                decode: |bytes| {
                    let versions: Vec<String> = serde_json::from_slice(bytes)?;
                    Ok(versions
                        .into_iter()
                        .map(|version| ForgeVersionEntry {
                            maven_coord: format!("1.20.4-{version}"),
                            mc_version: Some("1.20.4".into()),
                            loader_version: Some(version),
                        })
                        .collect())
                },
            }))
        }
    }

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();

    let mirror1 = MappedSource::new(
        "mirror1",
        vec![
            UrlMapping::new(
                "https://maven.minecraftforge.net/".parse().unwrap(),
                format!("http://{address}/mirror1/maven/").parse().unwrap(),
            )
            .unwrap(),
        ],
    )
    .unwrap()
    .with_catalog(Arc::new(LaggingMirrorCatalog {
        forge_versions_url: format!("http://{address}/mirror1/custom-forge/1.20.4")
            .parse()
            .unwrap(),
    }));
    let mirror2 = MappedSource::new(
        "mirror2",
        vec![
            UrlMapping::new(
                "https://maven.minecraftforge.net/".parse().unwrap(),
                format!("http://{address}/mirror2/maven/").parse().unwrap(),
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let sources: Vec<Arc<dyn DownloadSource>> =
        vec![Arc::new(mirror1) as Arc<dyn DownloadSource>, Arc::new(mirror2)];
    let downloader = Downloader::new(sources, DownloadOptions::default()).unwrap();

    let installer_bytes = b"forge-1.20.4-49.2.0-installer".to_vec();
    let sha1_hex = format!(
        "{:x}",
        <sha1::Sha1 as sha1::Digest>::digest(&installer_bytes)
    );
    let sha1_body = format!("{sha1_hex}  forge-1.20.4-49.2.0-installer.jar\r\n").into_bytes();
    let exchanges = vec![
        (
            "/mirror1/custom-forge/1.20.4".to_owned(),
            br#"["49.1.0"]"#.to_vec(),
        ),
        (
            "/mirror2/maven/net/minecraftforge/forge/maven-metadata.xml".to_owned(),
            br#"<metadata><versioning><versions>
                <version>1.20.4-49.1.0</version>
                <version>1.20.4-49.2.0</version>
            </versions></versioning></metadata>"#
                .to_vec(),
        ),
        (
            "/mirror1/maven/net/minecraftforge/forge/1.20.4-49.2.0/forge-1.20.4-49.2.0-installer.jar.sha1"
                .to_owned(),
            sha1_body,
        ),
        (
            "/mirror1/maven/net/minecraftforge/forge/1.20.4-49.2.0/forge-1.20.4-49.2.0-installer.jar"
                .to_owned(),
            b"corrupted-mirror-jar".to_vec(),
        ),
        (
            "/mirror2/maven/net/minecraftforge/forge/1.20.4-49.2.0/forge-1.20.4-49.2.0-installer.jar"
                .to_owned(),
            installer_bytes.clone(),
        ),
    ];

    let server = thread::spawn(move || {
        let mut visited = Vec::with_capacity(exchanges.len());
        for (expected_path, body) in exchanges {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "catalog fixture request timed out");
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("catalog fixture accept: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&buffer[..count]);
            }
            let head = String::from_utf8(request).unwrap();
            let first_line = head.lines().next().unwrap();
            let path = first_line.split_whitespace().nth(1).unwrap().to_owned();
            assert_eq!(path, expected_path);
            visited.push(path);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .unwrap();
            let _ = stream.write_all(&body);
        }
        visited
    });

    let rt = runtime();
    let release = rt
        .block_on(loader::resolve(
            &downloader,
            loader::LoaderFamily::Forge,
            "1.20.4",
            Some("49.2.0"),
        ))
        .unwrap();
    assert_eq!(release.family(), loader::LoaderFamily::Forge);
    assert_eq!(release.loader_version(), "49.2.0");

    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("forge-installer.jar");
    let outcome = rt
        .block_on(release.download_to(&downloader, &target))
        .unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), installer_bytes);
    assert_eq!(outcome.size, installer_bytes.len() as u64);

    let visited = server.join().unwrap();
    assert_eq!(visited.len(), 5);
}

#[test]
fn forge_promotions_failure_and_missing_promoted_version_fall_back_to_latest() {
    let sha1_hex = "0123456789abcdef0123456789abcdef01234567";
    let metadata_xml = br#"<metadata><versioning><versions>
        <version>1.20.4-49.1.0</version>
        <version>1.20.4-49.2.0</version>
    </versions></versioning></metadata>"#;

    for (promo_status, promo_body) in [
        ("500 Internal Server Error", b"upstream error".as_slice()),
        (
            "200 OK",
            br#"{"promos":{"1.20.4-recommended":"99.9.9"}}"#.as_slice(),
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();

        let mirror = MappedSource::new(
            "mirror",
            vec![
                UrlMapping::new(
                    "https://files.minecraftforge.net/".parse().unwrap(),
                    format!("http://{address}/files/").parse().unwrap(),
                )
                .unwrap(),
                UrlMapping::new(
                    "https://maven.minecraftforge.net/".parse().unwrap(),
                    format!("http://{address}/maven/").parse().unwrap(),
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let downloader = Downloader::new(
            vec![Arc::new(mirror) as Arc<dyn DownloadSource>],
            DownloadOptions::default(),
        )
        .unwrap();

        let exchanges = vec![
            (
                "/files/net/minecraftforge/forge/promotions_slim.json".to_owned(),
                promo_status.to_owned(),
                promo_body.to_vec(),
            ),
            (
                "/maven/net/minecraftforge/forge/maven-metadata.xml".to_owned(),
                "200 OK".to_owned(),
                metadata_xml.to_vec(),
            ),
            (
                "/maven/net/minecraftforge/forge/1.20.4-49.2.0/forge-1.20.4-49.2.0-installer.jar.sha1"
                    .to_owned(),
                "200 OK".to_owned(),
                format!("{sha1_hex}\n").into_bytes(),
            ),
        ];

        let server = thread::spawn(move || {
            for (expected_path, status, body) in exchanges {
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "promotions fixture timed out");
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(error) => panic!("promotions fixture accept: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).unwrap();
                    assert_ne!(count, 0);
                    request.extend_from_slice(&buffer[..count]);
                }
                let head = String::from_utf8(request).unwrap();
                let path = head.lines().next().unwrap().split_whitespace().nth(1).unwrap();
                assert_eq!(path, expected_path);
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .unwrap();
                let _ = stream.write_all(&body);
            }
        });

        let release = runtime()
            .block_on(loader::resolve(
                &downloader,
                loader::LoaderFamily::Forge,
                "1.20.4",
                None,
            ))
            .unwrap();
        assert_eq!(release.family(), loader::LoaderFamily::Forge);
        assert_eq!(release.loader_version(), "49.2.0");
        server.join().unwrap();
    }
}

fn request() -> DownloadRequest {
    DownloadRequest::new(
        "https://example.invalid/file".parse().unwrap(),
        None,
        vec![
            Checksum::new(
                ChecksumAlgorithm::Sha256,
                "d4f0bc5a29de06b510f9aa428f1eedba926012b591fef7a518e776a7c9bd1824",
            )
            .unwrap(),
        ],
    )
    .unwrap()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[derive(Debug, PartialEq)]
enum Recorded {
    Started(String),
    Bytes(u64, Option<u64>),
    Verifying,
    Failed,
    Completed,
}

impl From<Event<'_>> for Recorded {
    fn from(event: Event<'_>) -> Self {
        match event {
            Event::AttemptStarted { source, .. } => Self::Started(source.into()),
            Event::Bytes { received, total } => Self::Bytes(received, total),
            Event::Verifying => Self::Verifying,
            Event::AttemptFailed { .. } => Self::Failed,
            Event::Completed { .. } => Self::Completed,
        }
    }
}

/// Ordered responses let tests prove that fallback retains the same request and resets progress.
struct Fixture {
    downloader: Downloader,
    thread: thread::JoinHandle<()>,
}

impl Fixture {
    fn new(responses: Vec<(&[u8], bool)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let sources: Vec<Arc<dyn DownloadSource>> = ["first", "second"]
            .into_iter()
            .map(|name| {
                Arc::new(
                    MappedSource::new(
                        name,
                        vec![
                            UrlMapping::new(
                                "https://example.invalid/".parse().unwrap(),
                                format!("http://{address}/{name}/").parse().unwrap(),
                            )
                            .unwrap(),
                        ],
                    )
                    .unwrap(),
                ) as Arc<dyn DownloadSource>
            })
            .collect();
        let downloader = Downloader::new(sources, DownloadOptions::default()).unwrap();
        let responses: Vec<_> = responses
            .into_iter()
            .map(|(body, length)| (body.to_vec(), length))
            .collect();
        let thread = thread::spawn(move || {
            for (index, (body, length)) in responses.into_iter().enumerate() {
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "download request timed out");
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(error) => panic!("fixture accept: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).unwrap();
                    assert_ne!(count, 0);
                    request.extend_from_slice(&buffer[..count]);
                }
                let source = if index == 0 { "first" } else { "second" };
                assert!(
                    String::from_utf8(request)
                        .unwrap()
                        .starts_with(&format!("GET /{source}/file HTTP/1.1\r\n"))
                );
                write!(stream, "HTTP/1.1 200 OK\r\nConnection: close\r\n").unwrap();
                if length {
                    write!(stream, "Content-Length: {}\r\n", body.len()).unwrap();
                }
                stream.write_all(b"\r\n").unwrap();
                let _ = stream.write_all(&body);
            }
        });
        Self { downloader, thread }
    }
}
