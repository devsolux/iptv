use iptv_core::{
    db::Database,
    limits::LimitedRead,
    m3u, manifest,
    models::{ChannelQuery, ParsedChannel, SourceInput, StreamSecret},
    network::NetworkPolicy,
    vault::Vault,
    xmltv,
};
use std::{
    collections::BTreeMap,
    io::{BufReader, Cursor},
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};
use url::Url;
use uuid::Uuid;

struct Profile(PathBuf);
impl Profile {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("iptv-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn source() -> SourceInput {
    SourceInput {
        name: "Fixture".into(),
        playlist_url: "https://provider.invalid/list.m3u".into(),
        epg_urls: vec![],
        allow_http: false,
        allow_lan: false,
        cdn_hosts: vec![],
        accept_invalid_certs: false,
        country_code: "DE".into(),
        iptv_org: None,
    }
}
fn query() -> ChannelQuery {
    ChannelQuery {
        offset: 0,
        limit: 80,
        search: String::new(),
        source_id: None,
        group: None,
        country: None,
        favorites_only: false,
        revision: None,
    }
}
fn channel(url: &str) -> ParsedChannel {
    ParsedChannel {
        name: "News".into(),
        tvg_id: "news.id".into(),
        groups: vec!["News".into()],
        country: "DE TR".into(),
        stream: StreamSecret {
            url: url.into(),
            headers: BTreeMap::new(),
        },
        ..Default::default()
    }
}

#[test]
fn m3u_preserves_unicode_quoted_commas_and_provider_headers() {
    let text = "\u{feff}#EXTM3U\r\n#EXTINF:-1 tvg-id=\"news.id\" group-title=\"News, World\",Türkçe Haber\r\n#EXTVLCOPT:http-user-agent=FixtureAgent\r\nhttps://provider.invalid/live.m3u8|Referer=https%3A%2F%2Fprovider.invalid\r\n";
    let mut channels = vec![];
    let result = m3u::parse(Cursor::new(text), |channel| {
        channels.push(channel);
        Ok(())
    })
    .unwrap();
    assert_eq!(result.count, 1);
    assert_eq!(channels[0].name, "Türkçe Haber");
    assert_eq!(channels[0].tvg_id, "news.id");
    assert_eq!(
        channels[0].stream.headers.get("user-agent").unwrap(),
        "FixtureAgent"
    );
    assert_eq!(
        channels[0].stream.headers.get("referer").unwrap(),
        "https://provider.invalid"
    );
}
#[test]
fn invalid_empty_and_media_playlists_are_not_catalogues() {
    for input in [
        "not an m3u",
        "#EXTM3U\n",
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\nsegment.ts\n",
        "#EXTM3U\n#EXTINF:-1,Missing URL\n",
    ] {
        assert!(m3u::parse(Cursor::new(input), |_| Ok(())).is_err());
    }
}
#[test]
fn m3u_rejects_oversized_lines_before_accepting_a_channel() {
    let input = format!(
        "#EXTM3U\n#EXTINF:-1,{}\nhttps://provider.invalid/\n",
        "x".repeat(70_000)
    );
    assert!(m3u::parse(Cursor::new(input), |_| Ok(())).is_err());
}
#[test]
fn xmltv_normalizes_offsets_and_rejects_dtds() {
    assert_eq!(
        xmltv::timestamp("20261013080000 +0200").unwrap(),
        xmltv::timestamp("20261013060000 +0000").unwrap()
    );
    assert_eq!(
        xmltv::timestamp("20261013060000").unwrap(),
        xmltv::timestamp("20261013060000 +0000").unwrap()
    );
    assert!(xmltv::timestamp("20261013").is_err());
    assert!(
        xmltv::parse(
            Cursor::new("<!DOCTYPE tv [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><tv/>"),
            |_| Ok(())
        )
        .is_err()
    );
}
#[test]
fn xmltv_reads_channels_and_programmes_without_expanding_external_entities() {
    let xml = r#"<tv><channel id="news"><display-name>News &amp; More</display-name></channel><programme channel="news" start="20261013060000 +0000" stop="20261013070000 +0000"><title>Headlines</title><desc>Daily news.</desc></programme></tv>"#;
    let mut channels = 0;
    let mut programmes = 0;
    xmltv::parse(Cursor::new(xml), |record| {
        match record {
            xmltv::Record::Channel(_) => channels += 1,
            xmltv::Record::Programme(_) => programmes += 1,
        };
        Ok(())
    })
    .unwrap();
    assert_eq!((channels, programmes), (1, 1));
}
#[test]
fn device_key_store_is_persistent_context_bound_and_zeroized() {
    let profile = Profile::new();
    let vault = Vault::new(profile.0.join("key"));
    assert!(!vault.exists());
    vault.initialize().unwrap();
    assert!(vault.exists() && vault.is_ready());
    let sealed = vault.seal("source:a", b"private token").unwrap();
    assert!(!sealed.windows(13).any(|w| w == b"private token"));
    assert_eq!(
        vault.open("source:a", &sealed).unwrap().as_slice(),
        b"private token"
    );
    assert!(vault.open("source:b", &sealed).is_err());
    vault.clear();
    assert!(!vault.is_ready());
    assert!(vault.open("source:a", &sealed).is_err());
    // A second instance reads the same profile key and opens earlier records.
    let again = Vault::new(profile.0.join("key"));
    again.initialize().unwrap();
    assert_eq!(
        again.open("source:a", &sealed).unwrap().as_slice(),
        b"private token"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(profile.0.join("key"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // A damaged key file is rejected rather than silently replaced.
    std::fs::write(profile.0.join("key"), b"garbage").unwrap();
    assert!(Vault::new(profile.0.join("key")).initialize().is_err());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profiles_sealed_under_a_legacy_passphrase_are_reset_once() {
    let profile = Profile::new();
    let legacy = profile.0.join("credentials.vault");
    std::fs::write(&legacy, b"IPTVVAULT1 former passphrase vault").unwrap();
    {
        let db = Database::open(profile.0.join("catalogue.db")).unwrap();
        let vault = Vault::new(profile.0.join("other.key"));
        vault.initialize().unwrap();
        let id = db.save_source(&source(), &vault).unwrap();
        let mut import = db.importer(&id, &vault).unwrap();
        import
            .channel(channel("https://provider.invalid/a"))
            .unwrap();
        import.activate().unwrap();
        assert_eq!(db.sources().unwrap().len(), 1);
    }
    let core = iptv_core::Core::open(profile.0.clone()).await.unwrap();
    assert!(!legacy.exists());
    assert!(profile.0.join("credentials.key").exists());
    assert!(core.database.sources().unwrap().is_empty());
    assert_eq!(core.channels(&query()).unwrap().total, 0);
    // A fresh source works immediately: no prompt, no unlock step.
    let id = core.database.save_source(&source(), &core.vault).unwrap();
    assert_eq!(core.database.sources().unwrap()[0].id, id);
    core.shutdown();
}
#[test]
fn refresh_is_atomic_and_favorites_survive_stream_token_changes() {
    let profile = Profile::new();
    let db = Database::open(profile.0.join("catalogue.db")).unwrap();
    let vault = Vault::new(profile.0.join("vault"));
    vault.initialize().unwrap();
    let source = db.save_source(&source(), &vault).unwrap();
    let mut import = db.importer(&source, &vault).unwrap();
    import
        .channel(channel("https://provider.invalid/a?token=old"))
        .unwrap();
    import.activate().unwrap();
    let first = db.channels(&query()).unwrap();
    let id = first.items[0].id.clone();
    db.favorite(&id, true).unwrap();
    {
        let mut failed = db.importer(&source, &vault).unwrap();
        failed
            .channel(channel("https://provider.invalid/a?token=failed"))
            .unwrap();
    }
    assert_eq!(db.channels(&query()).unwrap().items[0].id, id);
    let mut next = db.importer(&source, &vault).unwrap();
    next.channel(channel("https://provider.invalid/a?token=new"))
        .unwrap();
    next.activate().unwrap();
    let result = db.channels(&query()).unwrap();
    assert_eq!(result.items[0].id, id);
    assert!(result.items[0].favorite);
    assert_eq!(db.countries(None).unwrap().len(), 2);
}
#[test]
fn request_headers_define_variants_not_new_logical_channels() {
    let profile = Profile::new();
    let db = Database::open(profile.0.join("catalogue.db")).unwrap();
    let vault = Vault::new(profile.0.join("vault"));
    vault.initialize().unwrap();
    let source = db.save_source(&source(), &vault).unwrap();
    let mut import = db.importer(&source, &vault).unwrap();
    let mut first = channel("https://provider.invalid/a");
    let mut second = first.clone();
    first.stream.headers.insert("user-agent".into(), "A".into());
    second
        .stream
        .headers
        .insert("user-agent".into(), "B".into());
    import.channel(first).unwrap();
    import.channel(second).unwrap();
    import.activate().unwrap();
    let result = db.channels(&query()).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].variants, 2);
}
#[test]
fn network_authority_does_not_include_other_hosts_or_metadata_endpoints() {
    let config = source();
    let policy = NetworkPolicy::new(&config.playlist_url, &config).unwrap();
    assert!(
        policy
            .check_url(&Url::parse("https://other.invalid/").unwrap())
            .is_err()
    );
    assert!(
        policy
            .check_url(&Url::parse("http://provider.invalid/").unwrap())
            .is_err()
    );
    for ip in [
        "127.0.0.1",
        "169.254.169.254",
        "10.0.0.1",
        "100.64.0.1",
        "::1",
        "fd00::1",
    ] {
        assert!(policy.check_ip(ip.parse().unwrap()).is_err());
    }
    let mut lan = config.clone();
    lan.allow_lan = true;
    let policy = NetworkPolicy::new(&lan.playlist_url, &lan).unwrap();
    assert!(policy.check_ip("192.168.1.2".parse().unwrap()).is_ok());
    assert!(policy.check_ip("127.0.0.1".parse().unwrap()).is_err());
}
#[test]
fn hls_rewrites_segments_keys_and_maps_without_passing_remote_urls_to_the_player() {
    let input = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4,\nsegment.m4s\n";
    let mut resources = vec![];
    let output = manifest::hls(
        input,
        &Url::parse("https://provider.invalid/live/list.m3u8").unwrap(),
        |url| {
            resources.push(url.to_owned());
            Ok(format!(
                "http://127.0.0.1:123/media/token/{}",
                resources.len()
            ))
        },
    )
    .unwrap();
    assert_eq!(resources.len(), 3);
    assert!(resources[0].ends_with("/live/key.bin"));
    assert!(!output.contains("https://provider.invalid"));
}
#[test]
fn dash_rewrites_relative_templates_and_rejects_drm() {
    let base = Url::parse("https://provider.invalid/live/manifest.mpd").unwrap();
    let clear = r#"<MPD><Period><AdaptationSet><SegmentTemplate media="chunk-$Number$.m4s" initialization="init.mp4"/><Representation id="v" bandwidth="1000"/></AdaptationSet></Period></MPD>"#;
    let mut resources = vec![];
    let result = manifest::dash(clear, &base, |url, _| {
        resources.push(url.to_owned());
        Ok(format!(
            "http://127.0.0.1:123/media/token/{}",
            resources.len()
        ))
    })
    .unwrap();
    assert!(resources.iter().any(|url| url.contains("$Number$")));
    assert!(result.contains("http://127.0.0.1"));
    assert!(
        manifest::dash("<MPD><ContentProtection/></MPD>", &base, |url, _| Ok(
            url.to_owned()
        ))
        .is_err()
    );
}
/// Registers like the relay: addresses outside the provider's HTTPS host are rejected.
fn provider_only(resources: &mut Vec<String>, url: &str) -> iptv_core::error::Result<String> {
    if !url.starts_with("https://provider.invalid/") {
        return Err(iptv_core::error::Error::Policy);
    }
    resources.push(url.to_owned());
    Ok(format!("http://127.0.0.1:123/media/s/{}", resources.len()))
}
#[test]
fn hls_drops_rejected_variants_and_renditions_but_keeps_the_rest() {
    let base = Url::parse("https://provider.invalid/live/master.m3u8").unwrap();
    let master = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"Other\",URI=\"http://mirror.invalid/a.m3u8\"\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"Main\",URI=\"audio.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=900000,AUDIO=\"a\"\nhttp://mirror.invalid/low.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2000000,AUDIO=\"a\"\n\nhigh.m3u8\n#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=1,URI=\"http://mirror.invalid/i.m3u8\"\n";
    let mut resources = vec![];
    let output = manifest::hls(master, &base, |url| provider_only(&mut resources, url)).unwrap();
    assert_eq!(resources.len(), 2, "{resources:?}");
    assert!(resources[0].ends_with("/live/audio.m3u8"));
    assert!(resources[1].ends_with("/live/high.m3u8"));
    assert!(!output.contains("mirror.invalid"));
    assert_eq!(output.matches("#EXT-X-STREAM-INF").count(), 1);
    assert!(!output.contains("BANDWIDTH=900000"));
    assert!(!output.contains("I-FRAME"));
    // The surviving variant keeps its URI on the line right after its tag (blank lines aside).
    let lines: Vec<&str> = output.lines().filter(|l| !l.is_empty()).collect();
    let info = lines
        .iter()
        .position(|l| l.starts_with("#EXT-X-STREAM-INF"))
        .unwrap();
    assert!(lines[info + 1].starts_with("http://127.0.0.1:123/"));

    let rejected = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nhttp://mirror.invalid/low.m3u8\n";
    assert!(matches!(
        manifest::hls(rejected, &base, |url| provider_only(&mut vec![], url)),
        Err(iptv_core::error::Error::Policy)
    ));
    // Media playlists cannot skip segments: a rejected segment fails the playlist.
    let media = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\nhttp://mirror.invalid/1.ts\n";
    assert!(manifest::hls(media, &base, |url| provider_only(&mut vec![], url)).is_err());
}
#[test]
fn hls_relays_identity_keys_and_rejects_drm_key_systems() {
    let base = Url::parse("https://provider.invalid/live/list.m3u8").unwrap();
    let playlist = |key: &str| format!("#EXTM3U\n{key}\n#EXTINF:4,\nsegment.ts\n");
    for key in [
        "#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"",
        "#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"key.bin\",KEYFORMAT=\"identity\"",
        "#EXT-X-KEY:METHOD=NONE",
    ] {
        assert!(
            manifest::hls(&playlist(key), &base, |url| Ok(url.to_owned())).is_ok(),
            "{key}"
        );
    }
    for key in [
        "#EXT-X-KEY:METHOD=SAMPLE-AES-CTR,URI=\"skd://key\"",
        "#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://key\",KEYFORMAT=\"com.apple.streamingkeydelivery\"",
        "#EXT-X-SESSION-KEY:METHOD=SAMPLE-AES,URI=\"data:x\",KEYFORMAT=\"urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed\"",
    ] {
        assert!(
            matches!(
                manifest::hls(&playlist(key), &base, |url| Ok(url.to_owned())),
                Err(iptv_core::error::Error::Unsupported)
            ),
            "{key}"
        );
    }
}
#[test]
fn dash_keeps_usable_clock_sources_and_marks_only_templates() {
    let base = Url::parse("https://provider.invalid/live/manifest.mpd").unwrap();
    let mpd = r#"<MPD type="dynamic"><UTCTiming schemeIdUri="urn:mpeg:dash:utc:http-xsdate:2014" value="https://provider.invalid/time"/><UTCTiming schemeIdUri="urn:mpeg:dash:utc:http-iso:2014" value="https://time.invalid/?iso"/><UTCTiming schemeIdUri="urn:mpeg:dash:utc:direct:2014" value="2026-10-16T10:00:00Z"/><UTCTiming schemeIdUri="urn:mpeg:dash:utc:ntp:2014" value="pool.ntp.org"/><Period><AdaptationSet><SegmentTemplate media="chunk-$Number$.m4s" initialization="init.mp4"/><Representation id="v" bandwidth="1"><SegmentList><SegmentURL media="part$1.m4s"/></SegmentList></Representation></AdaptationSet></Period></MPD>"#;
    let mut calls = vec![];
    let output = manifest::dash(mpd, &base, |url, template| {
        if !url.starts_with("https://provider.invalid/") {
            return Err(iptv_core::error::Error::Policy);
        }
        calls.push((url.to_owned(), template));
        Ok(format!("http://127.0.0.1:123/media/s/{}", calls.len()))
    })
    .unwrap();
    assert!(calls.contains(&("https://provider.invalid/time".to_owned(), false)));
    assert!(calls.contains(&(
        "https://provider.invalid/live/chunk-$Number$.m4s".to_owned(),
        true
    )));
    assert!(calls.contains(&("https://provider.invalid/live/init.mp4".to_owned(), true)));
    assert!(calls.contains(&("https://provider.invalid/live/part$1.m4s".to_owned(), false)));
    assert_eq!(output.matches("<UTCTiming").count(), 2, "{output}");
    assert!(output.contains("utc:direct:2014"));
    assert!(!output.contains("time.invalid") && !output.contains("ntp"));
}
#[test]
fn relay_detects_containers_from_content_before_names() {
    use iptv_core::relay::detect_kind;
    let mut ts = vec![0u8; 100];
    for _ in 0..8 {
        ts.push(0x47);
        ts.extend_from_slice(&[0u8; 187]);
    }
    assert_eq!(
        detect_kind(&ts, "", &["/live/1234"]),
        "mpegts",
        "unaligned start"
    );
    assert_eq!(
        detect_kind(&ts, "application/vnd.apple.mpegurl", &["/x.m3u8"]),
        "mpegts"
    );
    assert_eq!(detect_kind(b"FLV\x01\x05", "", &["/live"]), "flv");
    assert_eq!(
        detect_kind(b"\xef\xbb\xbf#EXTM3U\n", "video/mp2t", &["/a.ts"]),
        "hls"
    );
    assert_eq!(detect_kind(b"<?xml?><MPD>", "", &["/a"]), "dash");
    assert_eq!(detect_kind(b"", "application/x-mpegURL", &["/a"]), "hls");
    assert_eq!(
        detect_kind(b"\x1f\x8b", "", &["/a", "/b/index.M3U8"]),
        "hls"
    );
    assert_eq!(detect_kind(b"", "application/dash+xml", &["/a"]), "dash");
    assert_eq!(detect_kind(b"\x00\x01", "video/MP2T", &["/a"]), "mpegts");
    assert_eq!(
        detect_kind(b"\x00\x01", "", &["/live/channel.ts"]),
        "mpegts"
    );
    assert_eq!(
        detect_kind(b"\x00\x00\x00\x18ftypmp42", "video/mp4", &["/v.mp4"]),
        "progressive"
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relay_errors_carry_cors_headers_for_the_webview_only() {
    let relay = iptv_core::relay::Relay::bind().await.unwrap();
    let base = relay.logo_url("x");
    let base = base.split("/logo/").next().unwrap();
    let missing = format!("{base}/media/{}/{}", Uuid::new_v4(), Uuid::new_v4());
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = client
        .get(&missing)
        .header("origin", "http://tauri.localhost")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 410);
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .unwrap(),
        "http://tauri.localhost"
    );
    assert!(
        response.headers()["access-control-expose-headers"]
            .to_str()
            .unwrap()
            .contains("date")
    );
    let foreign = client
        .get(&missing)
        .header("origin", "https://example.invalid")
        .send()
        .await
        .unwrap();
    assert_eq!(foreign.status(), 403);
    assert!(
        foreign
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );
}
#[test]
fn cancelled_reads_fail_instead_of_spinning_in_the_xml_reader() {
    // quick-xml and std retry `Interrupted`; cancellation must surface as a hard error.
    let xml = r#"<tv><channel id="news"><display-name>News</display-name></channel></tv>"#;
    for limit in [0, 8, 1024] {
        let cancel = Arc::new(AtomicBool::new(true));
        let input = BufReader::new(LimitedRead::new(Cursor::new(xml), limit, cancel));
        assert!(xmltv::parse(input, |_| Ok(())).is_err());
    }
}
#[test]
fn an_open_import_does_not_block_concurrent_catalogue_writes() {
    let profile = Profile::new();
    let db = Database::open(profile.0.join("catalogue.db")).unwrap();
    let vault = Vault::new(profile.0.join("vault"));
    vault.initialize().unwrap();
    let source = db.save_source(&source(), &vault).unwrap();
    let mut import = db.importer(&source, &vault).unwrap();
    import
        .channel(channel("https://provider.invalid/a"))
        .unwrap();
    import.activate().unwrap();
    let id = db.channels(&query()).unwrap().items[0].id.clone();
    // A refresh is mid-download: rows are queued but the parser is waiting on the network.
    let mut refresh = db.importer(&source, &vault).unwrap();
    refresh
        .channel(channel("https://provider.invalid/a?token=new"))
        .unwrap();
    let started = std::time::Instant::now();
    db.favorite(&id, true).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    refresh.activate().unwrap();
    assert!(db.channels(&query()).unwrap().items[0].favorite);
}
#[test]
fn xmltv_rejects_non_utf8_text_and_attributes() {
    for xml in [
        &b"<tv><channel id=\"a\"><display-name>\xff</display-name></channel></tv>"[..],
        &b"<tv><channel id=\"\xff\"><display-name>A</display-name></channel></tv>"[..],
    ] {
        assert!(xmltv::parse(Cursor::new(xml), |_| Ok(())).is_err());
    }
}

// ------------------------------------------------------------------------------------------
// iptv-org: official playlist URLs, dataset ingestion, the selection matrix and the serializer.
// ------------------------------------------------------------------------------------------
use iptv_core::iptv_org::{self, Config, CountryMode, Method, OfficialPlaylist, PlaylistKind};

fn org_dataset() -> iptv_org::Dataset {
    iptv_org::load(|file| {
        let text = match file {
            "channels.json" => r#"[
 {"id":"CanNews.ca","name":"Can News","alt_names":[],"network":null,"owners":[],"country":"CA","categories":["news"],"is_nsfw":false,"launched":null,"closed":null,"replaced_by":null,"website":null},
 {"id":"DeNews.de","name":"DE News","country":"DE","categories":["news","general"],"is_nsfw":false,"closed":null},
 {"id":"Regional.de","name":"Regional","country":"DE","categories":["general"],"is_nsfw":false,"closed":null},
 {"id":"Nrw.de","name":"NRW","country":"DE","categories":null,"is_nsfw":false,"closed":null},
 {"id":"Nyc.us","name":"NYC","country":"US","categories":["news"],"is_nsfw":false,"closed":null},
 {"id":"Multi.de","name":"Multi","country":"DE","categories":["general"],"is_nsfw":false,"closed":null},
 {"id":"Blocked.de","name":"Blocked","country":"DE","categories":["news"],"is_nsfw":false,"closed":null},
 {"id":"Adult.de","name":"Adult","country":"DE","categories":["news"],"is_nsfw":true,"closed":null},
 {"id":"Closed.de","name":"Closed","country":"DE","categories":["news"],"is_nsfw":false,"closed":"2020-01-01"},
 {"id":"Geo.de","name":"Geo","country":"DE","categories":["news"],"is_nsfw":false,"closed":null},
 {"id":"Other.de","name":"Other","country":"DE","categories":["news"],"is_nsfw":false,"closed":null},
 {"id":"Brit.uk","name":"Brit","country":"UK","categories":["news"],"is_nsfw":false,"closed":null},
 {"id":"World.us","name":"World","country":"US","categories":["news"],"is_nsfw":false,"closed":null}
]"#,
            "feeds.json" => r#"[
 {"channel":"CanNews.ca","id":"HD","name":"HD","alt_names":[],"is_main":true,"broadcast_area":["c/DE"],"timezones":[],"languages":["deu"],"format":"1080p"},
 {"channel":"DeNews.de","id":"SD","name":"SD","is_main":true,"broadcast_area":["c/DE"],"languages":["deu"]},
 {"channel":"Regional.de","id":"EU","name":"Europe","is_main":false,"broadcast_area":["r/EU"],"languages":["deu"]},
 {"channel":"Nrw.de","id":"NW","name":"NW","is_main":true,"broadcast_area":["s/DE-NW"],"languages":["deu"]},
 {"channel":"Nyc.us","id":"NY","name":"New York","is_main":false,"broadcast_area":["ct/USNYC"],"languages":["eng"]},
 {"channel":"Multi.de","id":"A","name":"A","is_main":true,"broadcast_area":["c/DE"],"languages":["deu"]},
 {"channel":"Multi.de","id":"B","name":"B","is_main":false,"broadcast_area":["c/DE"],"languages":["deu"]},
 {"channel":"Blocked.de","id":"SD","name":"SD","is_main":true,"broadcast_area":["c/DE"],"languages":["deu"]},
 {"channel":"Adult.de","id":"SD","name":"SD","is_main":true,"broadcast_area":["c/DE"],"languages":["deu"]},
 {"channel":"Closed.de","id":"SD","name":"SD","is_main":true,"broadcast_area":["c/DE"],"languages":["deu"]},
 {"channel":"Geo.de","id":"SD","name":"SD","is_main":true,"broadcast_area":["c/DE"],"languages":["deu"]},
 {"channel":"Other.de","id":"SD","name":"SD","is_main":true,"broadcast_area":["c/DE"],"languages":["deu"]},
 {"channel":"Brit.uk","id":"SD","name":"SD","is_main":true,"broadcast_area":["c/UK"],"languages":["eng"]},
 {"channel":"World.us","id":"WW","name":"World","is_main":true,"broadcast_area":["r/WW"],"languages":["eng"]}
]"#,
            "streams.json" => r#"[
 {"channel":"CanNews.ca","feed":"HD","title":"Can News (720p)","url":"https://cdn-a.invalid/can720.m3u8","referrer":null,"user_agent":null,"quality":"720p","labels":[]},
 {"channel":"CanNews.ca","feed":"HD","title":"Can News (1080p)","url":"https://cdn-b.invalid/can1080.m3u8","referrer":"https://can.invalid/","user_agent":"Fixture/1.0","quality":"1080p","labels":[]},
 {"channel":"CanNews.ca","feed":"HD","title":"duplicate","url":"https://cdn-b.invalid/can1080.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":null},
 {"channel":"DeNews.de","feed":"SD","title":"DE News","url":"https://shared.invalid/live.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":[]},
 {"channel":"Other.de","feed":"SD","title":"Other","url":"https://shared.invalid/live.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":[]},
 {"channel":"Regional.de","feed":"EU","title":"Regional","url":"https://cdn.invalid/regional.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":[]},
 {"channel":"Nrw.de","feed":"NW","title":"NRW","url":"https://cdn.invalid/nrw.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":[]},
 {"channel":"Nyc.us","feed":"NY","title":"NYC","url":"https://cdn.invalid/nyc.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":[]},
 {"channel":"Multi.de","feed":null,"title":"Multi","url":"https://cdn.invalid/multi.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":[]},
 {"channel":"Blocked.de","feed":"SD","title":"Blocked","url":"https://cdn.invalid/blocked.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":[]},
 {"channel":"Adult.de","feed":"SD","title":"Adult","url":"https://cdn.invalid/adult.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":[]},
 {"channel":"Closed.de","feed":"SD","title":"Closed","url":"https://cdn.invalid/closed.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":[]},
 {"channel":"Geo.de","feed":"SD","title":"Geo","url":"https://cdn.invalid/geo.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":["Geo-blocked","Not 24/7"]},
 {"channel":"Brit.uk","feed":"SD","title":"Brit","url":"https://cdn.invalid/brit.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":[]},
 {"channel":"World.us","feed":"WW","title":"World","url":"https://cdn.invalid/world.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":[]},
 {"channel":null,"feed":null,"title":"Orphan","url":"https://cdn.invalid/orphan.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":[]},
 {"channel":"DeNews.de","feed":"SD","title":"rtmp","url":"rtmp://cdn.invalid/live","referrer":null,"user_agent":null,"quality":null,"labels":[]},
 {"channel":"DeNews.de","feed":"SD","title":"plain","url":"http://cdn.invalid/plain.m3u8","referrer":null,"user_agent":null,"quality":null,"labels":[]}
]"#,
            "logos.json" => r#"[
 {"channel":"CanNews.ca","feed":null,"in_use":true,"tags":[],"width":600,"height":300,"format":"PNG","url":"https://img.invalid/can-generic.png"},
 {"channel":"CanNews.ca","feed":"HD","in_use":true,"tags":[],"width":100,"height":100,"format":"PNG","url":"https://img.invalid/can-hd.png"},
 {"channel":"CanNews.ca","feed":"HD","in_use":true,"tags":[],"width":9000,"height":9000,"format":"PNG","url":"http://img.invalid/insecure.png"},
 {"channel":"DeNews.de","feed":null,"in_use":false,"tags":[],"width":600,"height":300,"format":"PNG","url":"https://img.invalid/de-old.png"}
]"#,
            "categories.json" => r#"[{"id":"news","name":"News","description":""},{"id":"general","name":"General","description":""}]"#,
            "countries.json" => r#"[{"code":"DE","name":"Germany","languages":["deu"],"flag":"🇩🇪"},{"code":"US","name":"United States","languages":["eng"],"flag":"🇺🇸"},{"code":"UK","name":"United Kingdom","languages":["eng"],"flag":"🇬🇧"},{"code":"CA","name":"Canada","languages":["eng","fra"],"flag":"🇨🇦"},{"code":"FR","name":"France","languages":["fra"],"flag":"🇫🇷"}]"#,
            "regions.json" => r#"[{"code":"EU","name":"European Union","countries":["DE","FR"]},{"code":"WW","name":"Worldwide","countries":["DE","US"]}]"#,
            "subdivisions.json" => r#"[{"country":"DE","code":"DE-NW","name":"Nordrhein-Westfalen","parent":null}]"#,
            "cities.json" => r#"[{"country":"US","subdivision":"US-NY","code":"USNYC","name":"New York","wikidata_id":"Q60"}]"#,
            "blocklist.json" => r#"[{"channel":"Blocked.de","reason":"dmca","ref":"https://example.invalid"}]"#,
            _ => "[]",
        };
        Ok(Box::new(Cursor::new(text.to_owned())))
    })
    .unwrap()
}
fn org_config(countries: &[&str], mode: CountryMode) -> Config {
    Config {
        api_base: String::new(),
        method: Method::Catalogue,
        playlists: vec![],
        geography: iptv_org::Geography {
            countries: countries.iter().map(|c| (*c).to_owned()).collect(),
            mode,
            ..Default::default()
        },
        filters: iptv_org::Filters {
            exclude_geo_blocked_label: false,
            exclude_not247_label: false,
            ..Default::default()
        },
        playlist: Default::default(),
        cache: Default::default(),
    }
}
fn org_source(config: Config) -> SourceInput {
    SourceInput {
        name: "iptv-org".into(),
        playlist_url: String::new(),
        epg_urls: vec![],
        allow_http: false,
        allow_lan: false,
        cdn_hosts: vec![],
        accept_invalid_certs: false,
        country_code: String::new(),
        iptv_org: Some(config),
    }
}
fn ids(entries: &[iptv_org::Entry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| format!("{}|{}", e.country, e.tvg_id))
        .collect()
}
fn https_only(url: &Url) -> bool {
    url.scheme() == "https"
}
const TODAY: &str = "2026-10-13";

#[test]
fn official_playlist_urls_follow_the_published_catalogue() {
    let base = Url::parse(iptv_org::DEFAULT_PLAYLIST_BASE).unwrap();
    let aliases = iptv_org::Geography::default().country_aliases;
    let url = |kind, code: &str| {
        iptv_org::official_url(
            &base,
            &OfficialPlaylist {
                kind,
                code: code.into(),
            },
            &aliases,
        )
    };
    let root = "https://iptv-org.github.io/iptv";
    assert_eq!(
        url(PlaylistKind::Country, "gb").unwrap(),
        (format!("{root}/countries/uk.m3u"), Some("UK".to_owned()))
    );
    assert_eq!(
        url(PlaylistKind::Country, "XK").unwrap().0,
        format!("{root}/countries/xk.m3u")
    );
    assert_eq!(
        url(PlaylistKind::Country, "int").unwrap(),
        (format!("{root}/countries/int.m3u"), None)
    );
    assert_eq!(
        url(PlaylistKind::Category, "News").unwrap().0,
        format!("{root}/categories/news.m3u")
    );
    assert_eq!(
        url(PlaylistKind::Language, "DEU").unwrap().0,
        format!("{root}/languages/deu.m3u")
    );
    assert_eq!(
        url(PlaylistKind::Region, "EUR").unwrap().0,
        format!("{root}/regions/eur.m3u")
    );
    assert_eq!(
        url(PlaylistKind::Subdivision, "GB-WLS").unwrap().0,
        format!("{root}/subdivisions/gb-wls.m3u")
    );
    assert_eq!(
        url(PlaylistKind::City, "USNYC").unwrap().0,
        format!("{root}/cities/usnyc.m3u")
    );
    assert_eq!(
        url(PlaylistKind::Source, "us_pluto").unwrap().0,
        format!("{root}/sources/us_pluto.m3u")
    );
    assert_eq!(
        url(PlaylistKind::Master, "").unwrap().0,
        format!("{root}/index.m3u")
    );
    for (kind, code) in [
        (PlaylistKind::Country, "DEU"),
        (PlaylistKind::Category, "bad slug"),
        (PlaylistKind::Language, "de"),
        (PlaylistKind::Source, "../x"),
        (PlaylistKind::Master, "x"),
        (PlaylistKind::Subdivision, "DENW"),
    ] {
        assert!(url(kind, code).is_err(), "{code}");
    }
    // An official playlist may legitimately be empty; a user playlist may not.
    assert!(m3u::parse_with(Cursor::new("#EXTM3U\n"), true, |_| Ok(())).is_ok());
    assert!(m3u::parse(Cursor::new("#EXTM3U\n"), |_| Ok(())).is_err());
}
#[test]
fn iptv_org_json_arrays_stream_items_and_enforce_limits() {
    let text = r#"[{"id":"a","name":"A","country":null,"categories":null},{"id":"b","name":"B","country":"DE"}]"#;
    let mut seen = vec![];
    let count = iptv_org::read_array::<iptv_org::Channel>(Cursor::new(text), 10, |c| {
        seen.push(format!("{}:{}", c.id, c.country));
        Ok(())
    })
    .unwrap();
    assert_eq!(count, 2);
    assert_eq!(seen, vec!["a:".to_owned(), "b:DE".to_owned()]);
    assert!(matches!(
        iptv_org::read_array::<iptv_org::Channel>(Cursor::new(text), 1, |_| Ok(())),
        Err(iptv_core::error::Error::Limit)
    ));
    assert!(matches!(
        iptv_org::read_array::<iptv_org::Channel>(Cursor::new(text), 10, |_| Err(
            iptv_core::error::Error::Cancelled
        )),
        Err(iptv_core::error::Error::Cancelled)
    ));
    for malformed in [r#"[{"id":"a"}]"#, "{}", "[{", "[] trailing"] {
        assert!(
            iptv_org::read_array::<iptv_org::Channel>(Cursor::new(malformed), 10, |_| Ok(()))
                .is_err(),
            "{malformed}"
        );
    }
}
#[test]
fn iptv_org_origin_and_coverage_modes_follow_the_documented_matrix() {
    let data = org_dataset();
    // Origin: the channel's own country decides; coverage and unresolved feeds are irrelevant.
    let origin = iptv_org::select(
        &data,
        &org_config(&["DE"], CountryMode::Origin),
        TODAY,
        https_only,
    )
    .unwrap();
    let origin_ids = ids(&origin);
    assert!(!origin_ids.iter().any(|id| id.contains("CanNews.ca")));
    for included in ["DE|DeNews.de@SD", "DE|Multi.de", "DE|Regional.de@EU"] {
        assert!(origin_ids.contains(&included.to_owned()), "{included}");
    }
    // Strict coverage: explicit c/, s/ and ct/ tokens include; r/EU, r/WW and ambiguous feeds do not.
    let strict = iptv_org::select(
        &data,
        &org_config(&["DE", "US"], CountryMode::CoverageStrict),
        TODAY,
        https_only,
    )
    .unwrap();
    let strict_ids = ids(&strict);
    for included in [
        "DE|CanNews.ca@HD",
        "DE|Nrw.de@NW",
        "US|Nyc.us@NY",
        "DE|Geo.de@SD",
    ] {
        assert!(strict_ids.contains(&included.to_owned()), "{included}");
    }
    for excluded in [
        "Regional.de",
        "Multi.de",
        "World.us",
        "Blocked.de",
        "Adult.de",
        "Closed.de",
        "Orphan",
        "Brit.uk",
    ] {
        assert!(
            !strict_ids.iter().any(|id| id.contains(excluded)),
            "{excluded}"
        );
    }
    // Alternatives: best reported quality first, duplicates removed, both kept as variants.
    let can: Vec<&iptv_org::Entry> = strict
        .iter()
        .filter(|e| e.tvg_id == "CanNews.ca@HD")
        .collect();
    assert_eq!(can.len(), 2);
    assert_eq!(can[0].url, "https://cdn-b.invalid/can1080.m3u8");
    assert_eq!(can[0].referrer.as_deref(), Some("https://can.invalid/"));
    assert_eq!(can[0].user_agent.as_deref(), Some("Fixture/1.0"));
    assert_eq!(can[0].logo, "https://img.invalid/can-hd.png");
    assert_eq!(can[0].group, "Germany / News");
    assert_eq!(can[0].name, "Can News");
    assert_eq!(can[1].name, "Can News");
    // The same URL under two distinct channels stays two entries; rejected schemes vanish.
    assert_eq!(
        strict
            .iter()
            .filter(|e| e.url == "https://shared.invalid/live.m3u8")
            .count(),
        2
    );
    assert!(strict.iter().all(|e| e.url.starts_with("https://")));
    // Regions: r/EU counts only in the regional mode and only for member countries.
    let regional = iptv_org::select(
        &data,
        &org_config(&["DE", "US"], CountryMode::CoverageWithRegions),
        TODAY,
        https_only,
    )
    .unwrap();
    let regional_ids = ids(&regional);
    assert!(regional_ids.contains(&"DE|Regional.de@EU".to_owned()));
    assert!(!regional_ids.contains(&"US|Regional.de@EU".to_owned()));
    assert!(!regional_ids.iter().any(|id| id.contains("World.us")));
    let regional_entry = regional
        .iter()
        .find(|e| e.tvg_id == "Regional.de@EU")
        .unwrap();
    assert_eq!(regional_entry.name, "Regional (Europe)");
    assert_eq!(regional_entry.group, "Germany / General");
    // Worldwide feeds join every requested country only when explicitly enabled.
    let mut international = org_config(&["DE", "US"], CountryMode::CoverageStrict);
    international.geography.include_international = true;
    let world = ids(&iptv_org::select(&data, &international, TODAY, https_only).unwrap());
    assert!(world.contains(&"DE|World.us@WW".to_owned()));
    assert!(world.contains(&"US|World.us@WW".to_owned()));
}
#[test]
fn iptv_org_filters_aliases_and_playlist_options_are_honoured() {
    let data = org_dataset();
    // GB is accepted and canonicalized to the published UK code; unknown countries are rejected.
    let uk = iptv_org::select(
        &data,
        &org_config(&["gb"], CountryMode::CoverageStrict),
        TODAY,
        https_only,
    )
    .unwrap();
    assert_eq!(ids(&uk), vec!["UK|Brit.uk@SD".to_owned()]);
    assert_eq!(uk[0].group, "United Kingdom / News");
    assert!(
        iptv_org::select(
            &data,
            &org_config(&["ZZ"], CountryMode::CoverageStrict),
            TODAY,
            https_only
        )
        .is_err()
    );
    let mut config = org_config(&["DE"], CountryMode::CoverageStrict);
    config.filters.exclude_blocked = false;
    config.filters.exclude_nsfw = false;
    config.filters.exclude_closed = false;
    config.filters.exclude_geo_blocked_label = true;
    config.filters.categories = vec!["news".into()];
    config.filters.languages = vec!["deu".into()];
    config.playlist.include_every_alternative_stream = false;
    config.playlist.group_by = vec![iptv_org::GroupBy::Category];
    let selected = iptv_org::select(&data, &config, TODAY, https_only).unwrap();
    let selected_ids = ids(&selected);
    for included in ["DE|Blocked.de@SD", "DE|Adult.de@SD", "DE|Closed.de@SD"] {
        assert!(selected_ids.contains(&included.to_owned()), "{included}");
    }
    for excluded in ["Geo.de", "Nrw.de", "Multi.de", "Nyc.us"] {
        assert!(
            !selected_ids.iter().any(|id| id.contains(excluded)),
            "{excluded}"
        );
    }
    assert_eq!(
        selected
            .iter()
            .filter(|e| e.tvg_id == "CanNews.ca@HD")
            .count(),
        1
    );
    assert!(selected.iter().all(|e| e.group == "News"));
    // The network policy decides which schemes are selected at all.
    let mut plain = org_config(&["DE"], CountryMode::CoverageStrict);
    plain.filters.exclude_not247_label = true;
    let any_scheme = iptv_org::select(&data, &plain, TODAY, |_| true).unwrap();
    assert!(
        any_scheme
            .iter()
            .any(|e| e.url == "http://cdn.invalid/plain.m3u8")
    );
    assert!(!any_scheme.iter().any(|e| e.url.starts_with("rtmp://")));
    assert!(!any_scheme.iter().any(|e| e.tvg_id == "Geo.de@SD"));
    // Label exclusions are on by default and official playlists carry them as name markers.
    let defaults = iptv_org::Filters::default();
    assert!(defaults.exclude_geo_blocked_label && defaults.exclude_not247_label);
    let mut standard = org_config(&["DE"], CountryMode::CoverageStrict);
    standard.filters = iptv_org::Filters::default();
    assert!(
        !ids(&iptv_org::select(&data, &standard, TODAY, https_only).unwrap())
            .iter()
            .any(|id| id.contains("Geo.de"))
    );
    assert!(defaults.excludes_official_name("360 (720p) [Not 24/7]"));
    assert!(defaults.excludes_official_name("+TV (576p) [Geo-blocked]"));
    assert!(!defaults.excludes_official_name("A Haber (1080p)"));
    assert!(
        !plain
            .filters
            .excludes_official_name("360 (720p) [Geo-blocked]")
    );
    // Configuration validation mirrors the contract.
    assert!(
        org_config(&["DE"], CountryMode::CoverageStrict)
            .validate()
            .is_ok()
    );
    assert!(
        org_config(&[], CountryMode::CoverageStrict)
            .validate()
            .is_err()
    );
    assert!(
        org_config(&["DEU"], CountryMode::CoverageStrict)
            .validate()
            .is_err()
    );
    let mut bad = org_config(&["DE"], CountryMode::CoverageStrict);
    bad.filters.languages = vec!["de".into()];
    assert!(bad.validate().is_err());
    let mut hours = org_config(&["DE"], CountryMode::CoverageStrict);
    hours.cache.metadata_refresh_hours = 0;
    assert!(hours.validate().is_err());
    let mut official = org_config(&[], CountryMode::CoverageStrict);
    official.method = Method::Playlist;
    assert!(official.validate().is_err());
    official.playlists = vec![OfficialPlaylist {
        kind: PlaylistKind::Country,
        code: "GB".into(),
    }];
    assert!(official.validate().is_ok());
}
#[test]
fn iptv_org_serializer_output_is_a_valid_catalogue_for_the_playlist_importer() {
    let data = org_dataset();
    let config = org_config(&["DE"], CountryMode::CoverageStrict);
    let entries = iptv_org::select(&data, &config, TODAY, https_only).unwrap();
    let guide = "https://guide.invalid/epg.xml".to_owned();
    let text = iptv_org::serialize(&entries, std::slice::from_ref(&guide), true);
    assert!(text.starts_with("#EXTM3U x-tvg-url=\"https://guide.invalid/epg.xml\"\n"));
    assert!(text.contains(
        "#EXTINF:-1 tvg-id=\"CanNews.ca@HD\" tvg-name=\"Can News\" tvg-logo=\"https://img.invalid/can-hd.png\" tvg-country=\"DE\" group-title=\"Germany / News\",Can News\n#EXTVLCOPT:http-referrer=https://can.invalid/\n#EXTVLCOPT:http-user-agent=Fixture/1.0\nhttps://cdn-b.invalid/can1080.m3u8\n"
    ));
    let mut parsed = vec![];
    let metadata = m3u::parse(Cursor::new(text.clone()), |channel| {
        parsed.push(channel);
        Ok(())
    })
    .unwrap();
    assert_eq!(metadata.count, entries.len());
    assert_eq!(metadata.suggested_epg_urls, vec![guide]);
    let can = parsed
        .iter()
        .find(|c| c.stream.url == "https://cdn-b.invalid/can1080.m3u8")
        .unwrap();
    assert_eq!(can.tvg_id, "CanNews.ca@HD");
    assert_eq!(can.country, "DE");
    assert_eq!(can.groups, vec!["Germany / News".to_owned()]);
    assert_eq!(
        can.stream.headers.get("referer").unwrap(),
        "https://can.invalid/"
    );
    assert_eq!(can.stream.headers.get("user-agent").unwrap(), "Fixture/1.0");
    // Without VLC options the playback hints are not emitted.
    assert!(!iptv_org::serialize(&entries, &[], false).contains("#EXTVLCOPT"));
    // Through the database: alternatives become variants of one logical channel.
    let profile = Profile::new();
    let db = Database::open(profile.0.join("catalogue.db")).unwrap();
    let vault = Vault::new(profile.0.join("vault"));
    vault.initialize().unwrap();
    let source = db.save_source(&org_source(config), &vault).unwrap();
    let mut import = db.importer(&source, &vault).unwrap();
    m3u::parse(Cursor::new(text), |channel| import.channel(channel)).unwrap();
    import.activate().unwrap();
    let page = db.channels(&query()).unwrap();
    assert_eq!(
        page.items.iter().filter(|c| c.name == "Can News").count(),
        1
    );
    assert_eq!(
        page.items
            .iter()
            .find(|c| c.name == "Can News")
            .unwrap()
            .variants,
        2
    );
    assert!(db.countries(None).unwrap().iter().any(|c| c.code == "DE"));
    assert_eq!(db.sources().unwrap()[0].kind, "iptv-org");
    // Quotes in names and attributes serialize into something the parser accepts verbatim.
    let quoted = iptv_org::Entry {
        tvg_id: "Q.de".into(),
        tvg_name: "Say \"hi\"".into(),
        logo: String::new(),
        group: "G".into(),
        country: "DE".into(),
        name: "Say \"hi\", now".into(),
        url: "https://cdn.invalid/q.m3u8".into(),
        referrer: None,
        user_agent: None,
    };
    let mut names = vec![];
    m3u::parse(
        Cursor::new(iptv_org::serialize(&[quoted], &[], true)),
        |c| {
            names.push((c.name, c.tvg_name));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        names,
        vec![("Say \"hi\", now".to_owned(), "Say 'hi'".to_owned())]
    );
}
#[test]
fn aggregator_sources_allow_public_hosts_but_keep_every_address_protection() {
    let source = org_source(org_config(&["DE"], CountryMode::CoverageStrict));
    assert!(source.is_aggregator());
    assert_eq!(source.kind(), "iptv-org");
    assert_eq!(source.refresh_interval_seconds(), 24 * 3600);
    let base = iptv_org::playlist_base(&source).unwrap();
    assert_eq!(base.as_str(), "https://iptv-org.github.io/iptv");
    let policy = NetworkPolicy::new(base.as_str(), &source).unwrap();
    assert!(
        policy
            .check_url(&Url::parse("https://any-cdn.invalid/live.m3u8").unwrap())
            .is_ok()
    );
    for rejected in [
        "http://any-cdn.invalid/live.m3u8",
        "https://user:pw@any-cdn.invalid/",
        "https://127.0.0.1/",
        "https://169.254.169.254/latest",
        "https://10.0.0.1/",
        "https://[::1]/",
    ] {
        assert!(
            policy.check_url(&Url::parse(rejected).unwrap()).is_err(),
            "{rejected}"
        );
    }
    let public = NetworkPolicy::public("https://iptv-org.github.io/api").unwrap();
    assert!(
        public
            .check_url(&Url::parse("https://other.invalid/").unwrap())
            .is_err()
    );
    assert!(NetworkPolicy::public("http://iptv-org.github.io/api").is_err());
    assert!(iptv_core::network::validate_source(&source).is_ok());
    let mut lan = source.clone();
    lan.allow_lan = true;
    assert!(iptv_core::network::validate_source(&lan).is_err());
    let mut cdn = source.clone();
    cdn.cdn_hosts = vec!["cdn.invalid".into()];
    assert!(iptv_core::network::validate_source(&cdn).is_err());
    let mut mirror = source.clone();
    mirror.playlist_url = "http://mirror.invalid/iptv".into();
    assert!(iptv_core::network::validate_source(&mirror).is_err());
    mirror.playlist_url = "https://mirror.invalid/iptv/".into();
    assert!(iptv_core::network::validate_source(&mirror).is_ok());
    // Own M3U sources keep the exact host allowlist and the six-hour refresh cadence.
    let own = source_fixture();
    assert!(!own.is_aggregator());
    assert_eq!(own.kind(), "m3u");
    assert_eq!(own.refresh_interval_seconds(), 6 * 3600);
    let own_policy = NetworkPolicy::new(&own.playlist_url, &own).unwrap();
    assert!(
        own_policy
            .check_url(&Url::parse("https://any-cdn.invalid/live.m3u8").unwrap())
            .is_err()
    );
}
fn source_fixture() -> SourceInput {
    source()
}
#[test]
fn version_one_profiles_migrate_and_record_the_source_kind() {
    let profile = Profile::new();
    let path = profile.0.join("catalogue.db");
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!("../migrations/001_initial.sql"))
            .unwrap();
        connection
            .execute(
                "INSERT INTO sources(id,name,config) VALUES('11111111-1111-4111-8111-111111111111','Legacy',X'0102')",
                [],
            )
            .unwrap();
    }
    let db = Database::open(path.clone()).unwrap();
    assert_eq!(db.sources().unwrap()[0].kind, "m3u");
    let version: i64 = rusqlite::Connection::open(&path)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 3);
    let vault = Vault::new(profile.0.join("vault"));
    vault.initialize().unwrap();
    db.save_source(&source(), &vault).unwrap();
    db.save_source(
        &org_source(org_config(&["DE"], CountryMode::CoverageStrict)),
        &vault,
    )
    .unwrap();
    let mut kinds: Vec<String> = db.sources().unwrap().into_iter().map(|s| s.kind).collect();
    kinds.sort();
    assert_eq!(kinds, vec!["iptv-org", "m3u", "m3u"]);
}
/// Live check against the published iptv-org data; run explicitly with `--ignored`.
#[test]
#[ignore = "requires network access to iptv-org.github.io"]
fn live_iptv_org_datasets_parse_select_and_official_playlists_import() {
    let api = NetworkPolicy::public(iptv_org::DEFAULT_API_BASE).unwrap();
    let fetch = |file: &str| -> iptv_core::error::Result<Box<dyn std::io::Read>> {
        let response = api.get_blocking(
            &format!("{}/{file}", iptv_org::DEFAULT_API_BASE),
            &BTreeMap::new(),
        )?;
        Ok(Box::new(BufReader::new(response)))
    };
    let data = iptv_org::load(fetch).unwrap();
    assert!(data.channels.len() > 1000 && data.feeds.len() > 1000 && data.streams.len() > 1000);
    assert!(data.countries.iter().any(|c| c.code == "UK"));
    let mut config = org_config(&["DE", "TR"], CountryMode::CoverageStrict);
    config.filters.categories = vec!["news".into()];
    let entries = iptv_org::select(&data, &config, TODAY, https_only).unwrap();
    assert!(!entries.is_empty());
    assert!(
        entries
            .iter()
            .all(|e| e.country == "DE" || e.country == "TR")
    );
    let text = iptv_org::serialize(&entries, &[], true);
    let mut count = 0;
    m3u::parse(Cursor::new(text), |_| {
        count += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(count, entries.len());
    let catalog =
        iptv_org::load_catalog(fetch, &Url::parse(iptv_org::DEFAULT_PLAYLIST_BASE).unwrap())
            .unwrap();
    assert!(
        catalog
            .countries
            .iter()
            .any(|c| c.code == "UK" && c.playlist_candidate_url.ends_with("/countries/uk.m3u"))
    );
    assert!(catalog.categories.iter().any(|c| c.id == "news"));
    assert!(catalog.languages.iter().any(|l| l.code == "tur"));
    assert!(catalog.regions.iter().any(|r| r.code == "EUR"));
    // Official playlists parse with the ordinary importer.
    let source = org_source(config);
    let base = iptv_org::playlist_base(&source).unwrap();
    let policy = NetworkPolicy::new(base.as_str(), &source).unwrap();
    let (url, country) = iptv_org::official_url(
        &base,
        &OfficialPlaylist {
            kind: PlaylistKind::Country,
            code: "gb".into(),
        },
        &source.iptv_org.as_ref().unwrap().geography.country_aliases,
    )
    .unwrap();
    assert_eq!(country.as_deref(), Some("UK"));
    let response = policy.get_blocking(&url, &BTreeMap::new()).unwrap();
    let mut official = 0;
    let meta = m3u::parse_with(BufReader::new(response), true, |_| {
        official += 1;
        Ok(())
    })
    .unwrap();
    assert!(official > 0 && meta.count == official);
    eprintln!(
        "live: {} channels, {} feeds, {} streams, {} selected DE/TR news entries, {} official UK entries",
        data.channels.len(),
        data.feeds.len(),
        data.streams.len(),
        entries.len(),
        official
    );
}
#[test]
fn artwork_is_sniffed_cached_and_served_only_through_the_image_policy() {
    use iptv_core::logos;
    assert_eq!(
        logos::image_type(b"\x89PNG\r\n\x1a\n....", "text/plain"),
        Some("image/png")
    );
    assert_eq!(
        logos::image_type(b"\xff\xd8\xff\xe0rest", ""),
        Some("image/jpeg")
    );
    assert_eq!(logos::image_type(b"GIF89a....", ""), Some("image/gif"));
    assert_eq!(
        logos::image_type(
            b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>",
            "image/svg+xml"
        ),
        Some("image/svg+xml")
    );
    assert_eq!(
        logos::image_type(b"<svg><script>alert(1)</script></svg>", "image/svg+xml"),
        None
    );
    assert_eq!(
        logos::image_type(b"<html><svg/></html>", "image/svg+xml"),
        None
    );
    let profile = Profile::new();
    let path = profile.0.join("logo");
    logos::write_cached(&path, "image/png", b"\x89PNG\r\n\x1a\nbody").unwrap();
    assert_eq!(
        logos::read_cached(&path),
        Some((b"\x89PNG\r\n\x1a\nbody".to_vec(), "image/png"))
    );
    std::fs::write(&path, b"text/html\n<html>").unwrap();
    assert_eq!(logos::read_cached(&path), None);
    // Image policy: any public host, the source's transport consents, never private addresses.
    let own = source();
    let policy = NetworkPolicy::images(&own);
    assert!(
        policy
            .check_url(&Url::parse("https://images.invalid/logo.png").unwrap())
            .is_ok()
    );
    assert!(
        policy
            .check_url(&Url::parse("http://images.invalid/logo.png").unwrap())
            .is_err()
    );
    assert!(
        policy
            .check_url(&Url::parse("https://10.0.0.1/logo.png").unwrap())
            .is_err()
    );
    assert!(!policy.accept_invalid_certs);
    // The stored artwork URL is readable only for active channels, through the vault.
    let db = Database::open(profile.0.join("catalogue.db")).unwrap();
    let vault = Vault::new(profile.0.join("vault"));
    vault.initialize().unwrap();
    let source_id = db.save_source(&own, &vault).unwrap();
    let mut import = db.importer(&source_id, &vault).unwrap();
    let mut with_logo = channel("https://provider.invalid/a");
    with_logo.logo = "https://images.invalid/news.png".into();
    import.channel(with_logo).unwrap();
    let mut without = channel("https://provider.invalid/b");
    without.name = "Plain".into();
    without.tvg_id = "plain.id".into();
    import.channel(without).unwrap();
    import.activate().unwrap();
    let page = db.channels(&query()).unwrap();
    let news = page.items.iter().find(|c| c.name == "News").unwrap();
    let plain = page.items.iter().find(|c| c.name == "Plain").unwrap();
    assert!(news.has_logo && !plain.has_logo);
    assert!(news.logo_url.is_none());
    let (url, config) = db.channel_logo(&news.id, &vault).unwrap();
    assert_eq!(url, "https://images.invalid/news.png");
    assert_eq!(config.name, "Fixture");
    assert!(matches!(
        db.channel_logo(&plain.id, &vault),
        Err(iptv_core::error::Error::NotFound)
    ));
}

/// Live check: a public iptv-org stream plays through the loopback relay with the production
/// client settings (default agent, cookies, HTTP/1.1, manifest rewriting). Run with `--ignored`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires network access to iptv-org.github.io and public stream hosts"]
async fn live_relay_serves_a_public_iptv_org_stream() {
    let candidates = tokio::task::spawn_blocking(|| {
        let policy = NetworkPolicy::public(iptv_org::DEFAULT_PLAYLIST_BASE).unwrap();
        let response = policy
            .get_blocking(
                &format!("{}/countries/de.m3u", iptv_org::DEFAULT_PLAYLIST_BASE),
                &BTreeMap::new(),
            )
            .unwrap();
        let mut streams = vec![];
        m3u::parse_with(BufReader::new(response), true, |channel| {
            if channel.stream.url.starts_with("https://") && streams.len() < 12 {
                streams.push(channel.stream);
            }
            Ok(())
        })
        .unwrap();
        streams
    })
    .await
    .unwrap();
    assert!(!candidates.is_empty());
    let source = org_source(org_config(&["DE"], CountryMode::CoverageStrict));
    let relay = iptv_core::relay::Relay::bind().await.unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut played = None;
    for secret in candidates {
        let host = Url::parse(&secret.url)
            .unwrap()
            .host_str()
            .unwrap()
            .to_owned();
        let prepared = match relay.start(secret, &source).await {
            Ok(prepared) => prepared,
            Err(error) => {
                eprintln!("live: {host}: {error}");
                continue;
            }
        };
        let response = client.get(&prepared.url).send().await.unwrap();
        if !response.status().is_success() {
            eprintln!("live: {host}: relay answered {}", response.status());
            continue;
        }
        let body = response.text().await.unwrap();
        if prepared.kind == "hls" {
            assert!(body.starts_with("#EXTM3U"));
            let next = body
                .lines()
                .find(|line| !line.starts_with('#') && !line.trim().is_empty())
                .map(str::to_owned);
            if let Some(next) = next {
                assert!(next.starts_with("http://127.0.0.1:"));
                let segment = client.get(&next).send().await.unwrap();
                if !segment.status().is_success() {
                    eprintln!("live: {host}: segment answered {}", segment.status());
                    continue;
                }
            }
        }
        played = Some((host, prepared.kind));
        break;
    }
    let (host, kind) = played.expect("no public stream could be relayed");
    eprintln!("live: relayed a {kind} stream from {host}");
    relay.close(None);
}

/// Live check: an official iptv-org playlist imports inside a running `Core` and channel
/// artwork is served through the relay's logo route. Run with `--ignored`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires network access to iptv-org.github.io and public image hosts"]
async fn live_core_imports_an_official_playlist_and_serves_artwork() {
    let profile = Profile::new();
    let core = iptv_core::Core::open(profile.0.clone()).await.unwrap();
    let mut config = org_config(&[], CountryMode::CoverageStrict);
    config.method = Method::Playlist;
    config.playlists = vec![OfficialPlaylist {
        kind: PlaylistKind::Country,
        code: "de".into(),
    }];
    let mut source = org_source(config);
    source.allow_http = true;
    let source_id = core.database.save_source(&source, &core.vault).unwrap();
    let job = core.start_job(source_id).unwrap();
    let started = std::time::Instant::now();
    loop {
        let status = core.job(&job).unwrap();
        match status.state.as_str() {
            "completed" => break,
            "failed" | "cancelled" => panic!("import {}: {:?}", status.state, status.error),
            _ => {}
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(180),
            "import timed out"
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let page = core.channels(&query()).unwrap();
    assert!(page.total > 0);
    let with_logo = page
        .items
        .iter()
        .find(|c| c.logo_url.is_some())
        .expect("official playlists carry tvg-logo");
    let logo_url = with_logo.logo_url.clone().unwrap();
    assert!(logo_url.starts_with("http://127.0.0.1:") && logo_url.contains("/logo/"));
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = client.get(&logo_url).send().await.unwrap();
    assert_eq!(response.status(), 200, "logo route");
    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(content_type.starts_with("image/"), "{content_type}");
    assert_eq!(
        response.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    let bytes = response.bytes().await.unwrap();
    assert!(!bytes.is_empty());
    // Second request is served from the private cache without a network round trip.
    let again = core.logo(&with_logo.id).await.unwrap();
    assert_eq!(again.0.len(), bytes.len());
    assert!(std::fs::read_dir(profile.0.join("logos")).unwrap().count() >= 1);
    // A wrong token or an unknown channel never leaks anything.
    let wrong = logo_url.replace("/logo/", "/logo/x");
    assert_ne!(client.get(&wrong).send().await.unwrap().status(), 200);
    eprintln!(
        "live: imported {} channels, artwork {} ({} bytes)",
        page.total,
        content_type,
        bytes.len()
    );
    core.shutdown();
}
/// Audit: drives the relay the way a native player does for a sample of public streams and
/// prints one line per stream. Run with `--ignored --nocapture`; it asserts nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "audit: requires network access to iptv-org.github.io and public stream hosts"]
async fn live_relay_audit_of_public_streams() {
    let candidates = tokio::task::spawn_blocking(|| {
        let policy = NetworkPolicy::public(iptv_org::DEFAULT_PLAYLIST_BASE).unwrap();
        let mut streams = vec![];
        for country in ["de", "tr", "us"] {
            let response = policy
                .get_blocking(
                    &format!(
                        "{}/countries/{country}.m3u",
                        iptv_org::DEFAULT_PLAYLIST_BASE
                    ),
                    &BTreeMap::new(),
                )
                .unwrap();
            let mut taken = 0;
            m3u::parse_with(BufReader::new(response), true, |channel| {
                if taken < 12 {
                    streams.push((channel.name, channel.stream));
                    taken += 1;
                }
                Ok(())
            })
            .unwrap();
        }
        streams
    })
    .await
    .unwrap();
    let mut source = org_source(org_config(&["DE"], CountryMode::CoverageStrict));
    source.allow_http = true;
    let relay = iptv_core::relay::Relay::bind().await.unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap();
    let mut summary = std::collections::BTreeMap::<String, usize>::new();
    let first_line = |text: &str| {
        text.lines()
            .find(|l| !l.starts_with('#') && !l.trim().is_empty())
            .map(str::to_owned)
    };
    let kind_of = |bytes: &[u8]| {
        if bytes.first() == Some(&0x47) {
            "ts"
        } else if bytes.len() > 8
            && (&bytes[4..8] == b"ftyp" || bytes.windows(4).any(|w| w == b"moof"))
        {
            "fmp4"
        } else {
            "?"
        }
    };
    for (name, secret) in candidates {
        let host = Url::parse(&secret.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default();
        let prepared = match relay.start(secret, &source).await {
            Ok(prepared) => prepared,
            Err(error) => {
                eprintln!("audit | {name} | {host} | start: {error}");
                *summary.entry(format!("start: {error}")).or_default() += 1;
                continue;
            }
        };
        let (status, body) = match client.get(&prepared.url).send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                (status, response.text().await.unwrap_or_default())
            }
            Err(error) => {
                eprintln!("audit | {name} | {host} | root transport: {error}");
                continue;
            }
        };
        let ranged = client
            .get(&prepared.url)
            .header("range", "bytes=0-1")
            .send()
            .await
            .map(|r| r.status().as_u16())
            .unwrap_or(0);
        let head = client
            .head(&prepared.url)
            .send()
            .await
            .map(|r| r.status().as_u16())
            .unwrap_or(0);
        let mut detail = String::new();
        if prepared.kind == "hls" && status == 200 {
            let master = body.contains("#EXT-X-STREAM-INF");
            let codecs: Vec<String> = body
                .lines()
                .filter(|l| l.starts_with("#EXT-X-STREAM-INF"))
                .filter_map(|l| {
                    l.split("CODECS=\"")
                        .nth(1)
                        .and_then(|r| r.split('"').next())
                        .map(str::to_owned)
                })
                .collect();
            let target = body.contains("#EXT-X-TARGETDURATION");
            let version = body.contains("#EXT-X-VERSION");
            let mut media_status = 0;
            let mut media_target = false;
            let mut segment_status = 0;
            let mut segment_kind = "-";
            if let Some(next) = first_line(&body) {
                let media = if master {
                    match client.get(&next).send().await {
                        Ok(response) => {
                            media_status = response.status().as_u16();
                            response.text().await.unwrap_or_default()
                        }
                        Err(_) => String::new(),
                    }
                } else {
                    media_status = status;
                    body.clone()
                };
                media_target = media.contains("#EXT-X-TARGETDURATION");
                if let Some(segment) = first_line(&media)
                    && let Ok(response) = client
                        .get(&segment)
                        .header("range", "bytes=0-1023")
                        .send()
                        .await
                {
                    segment_status = response.status().as_u16();
                    let bytes = response.bytes().await.unwrap_or_default();
                    segment_kind = kind_of(&bytes);
                }
            }
            detail = format!(
                "master={master} version={version} target={target}/{media_target} codecs={codecs:?} media={media_status} segment={segment_status}/{segment_kind}"
            );
        }
        eprintln!(
            "audit | {name} | {host} | kind={} root={status} range={ranged} head={head} {detail}",
            prepared.kind
        );
        *summary
            .entry(format!(
                "kind={} root={status} range={ranged} head={head}",
                prepared.kind
            ))
            .or_default() += 1;
        relay.close(Some(&prepared.id));
    }
    for (key, count) in summary {
        eprintln!("summary | {count} | {key}");
    }
}

#[test]
fn relay_recognizes_playlists_by_name_and_content() {
    use iptv_core::relay::{manifest_prefix, playlist_like};
    for url in [
        "https://host.invalid/live/index.m3u8",
        "https://host.invalid/list.M3U8?token=1",
        "https://host.invalid/stream.m3u",
        "https://host.invalid/dash/manifest.mpd",
    ] {
        assert!(playlist_like(&Url::parse(url).unwrap()), "{url}");
    }
    for url in [
        "https://host.invalid/segment-1.ts",
        "https://host.invalid/init.mp4",
        "https://host.invalid/live/stream",
    ] {
        assert!(!playlist_like(&Url::parse(url).unwrap()), "{url}");
    }
    assert_eq!(
        manifest_prefix("\u{feff}  #EXTM3U\n#EXT-X-VERSION:3"),
        Some("hls")
    );
    assert_eq!(
        manifest_prefix("<?xml version=\"1.0\"?><MPD>"),
        Some("dash")
    );
    assert_eq!(manifest_prefix("#E"), None);
    assert_eq!(manifest_prefix("\x47\x40\x00"), None);
}
