use iptv_core::{
    db::Database,
    m3u, manifest,
    models::{ChannelQuery, ParsedChannel, SourceInput, StreamSecret},
    network::NetworkPolicy,
    vault::Vault,
    xmltv,
};
use std::{collections::BTreeMap, io::Cursor, path::PathBuf};
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

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
        country_code: "DE".into(),
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
fn vault_enforces_context_authentication_and_locking() {
    let profile = Profile::new();
    let vault = Vault::new(profile.0.join("vault"));
    vault
        .unlock(Zeroizing::new("a long test passphrase".to_owned()))
        .unwrap();
    let sealed = vault.seal("source:a", b"private token").unwrap();
    assert!(!sealed.windows(13).any(|w| w == b"private token"));
    assert_eq!(
        vault.open("source:a", &sealed).unwrap().as_slice(),
        b"private token"
    );
    assert!(vault.open("source:b", &sealed).is_err());
    vault.lock();
    assert!(vault.open("source:a", &sealed).is_err());
    assert!(
        vault
            .unlock(Zeroizing::new("incorrect passphrase".to_owned()))
            .is_err()
    );
    vault
        .unlock(Zeroizing::new("a long test passphrase".to_owned()))
        .unwrap();
}
#[test]
fn refresh_is_atomic_and_favorites_survive_stream_token_changes() {
    let profile = Profile::new();
    let db = Database::open(profile.0.join("catalogue.db")).unwrap();
    let vault = Vault::new(profile.0.join("vault"));
    vault
        .unlock(Zeroizing::new("a long test passphrase".into()))
        .unwrap();
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
    vault
        .unlock(Zeroizing::new("a long test passphrase".into()))
        .unwrap();
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
    let result = manifest::dash(clear, &base, |url| {
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
        manifest::dash("<MPD><ContentProtection/></MPD>", &base, |url| Ok(
            url.to_owned()
        ))
        .is_err()
    );
}
