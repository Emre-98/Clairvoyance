//! v1.8 encoder options that don't depend on Windows: which codec a recording really uses,
//! its bitrate relative to H.264 and the quality-based mode's value (unit-tested on Linux).

/// Bitrate of the same quality in another codec, relative to H.264 (hardware encoders,
/// Medal's published guidance: 1080p H.264 15-20, HEVC 10-15, AV1 7-10 Mbps). Kept on the
/// safe side until `scripts/encoder-compare.ps1` measures this PC's encoder.
pub fn codec_factor(c: crate::mp4::Codec) -> f64 {
    match c {
        crate::mp4::Codec::H264 => 1.0,
        crate::mp4::Codec::Hevc => 0.75,
        crate::mp4::Codec::Av1 => 0.6,
    }
}

/// Media Foundation "Quality" value (0-100) for the quality setting in quality-based mode.
pub fn quality_value(quality: &str) -> u32 {
    if quality == "high" {
        80
    } else {
        70
    }
}

/// The codec actually used: the chosen one only when the in-app player can play it and
/// Windows can decode it (thumbnails, the ult / summoner check); H.264 otherwise.
pub fn pick_codec(chosen: &str, playable: &[String], decodable: impl Fn(crate::mp4::Codec) -> bool) -> (crate::mp4::Codec, Option<String>) {
    use crate::mp4::Codec;
    let want = Codec::parse(chosen).unwrap_or(Codec::H264);
    if want == Codec::H264 {
        return (Codec::H264, None);
    }
    if !playable.iter().any(|p| p == want.as_str()) {
        return (Codec::H264, Some(format!("{} isn't playable in the app's player on this PC: recording in H.264", want.as_str().to_uppercase())));
    }
    if !decodable(want) {
        let ext = if want == Codec::Hevc { "HEVC Video Extensions" } else { "AV1 Video Extension" };
        return (Codec::H264, Some(format!("Windows can't decode {} (install Microsoft's {ext}): recording in H.264", want.as_str().to_uppercase())));
    }
    (want, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp4::Codec;

    #[test]
    fn codec_only_when_playable_and_decodable() {
        let all = vec!["h264".to_string(), "hevc".into(), "av1".into()];
        assert_eq!(pick_codec("h264", &[], |_| false), (Codec::H264, None));
        assert_eq!(pick_codec("", &all, |_| true).0, Codec::H264, "unset = H.264");
        assert_eq!(pick_codec("hevc", &all, |_| true), (Codec::Hevc, None));
        let (c, why) = pick_codec("hevc", &["h264".into()], |_| true);
        assert_eq!(c, Codec::H264);
        assert!(why.unwrap().contains("player"));
        let (c, why) = pick_codec("hevc", &all, |c| c != Codec::Hevc);
        assert_eq!(c, Codec::H264);
        assert!(why.unwrap().contains("HEVC Video Extensions"));
        let (c, why) = pick_codec("av1", &all, |c| c == Codec::H264);
        assert_eq!(c, Codec::H264);
        assert!(why.unwrap().contains("AV1 Video Extension"));
        assert_eq!(pick_codec("AV1", &all, |_| true).0, Codec::Av1);
    }

    #[test]
    fn factors() {
        assert_eq!(codec_factor(Codec::H264), 1.0);
        assert!(codec_factor(Codec::Hevc) < 1.0 && codec_factor(Codec::Av1) < codec_factor(Codec::Hevc));
        assert!(quality_value("high") > quality_value("standard"));
    }
}
