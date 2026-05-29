use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub static SUPPORTED_FORMATS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        ".mp3", ".flac", ".wav", ".m4a", ".aac", ".ogg", ".wma", ".ape", ".opus",
        ".alac", ".aiff", ".wv",
    ])
});

pub fn can_convert(filename: &str) -> bool {
    let name_lower = filename.to_lowercase();
    SUPPORTED_FORMATS
        .iter()
        .any(|ext| name_lower.ends_with(ext))
}

pub fn convert_bitrate(
    input_path: &str,
    output_path: &str,
    bitrate_kbps: u32,
) -> Result<String, String> {
    if bitrate_kbps == 0 {
        return copy_file(input_path, output_path);
    }
    transcode_to_mp3(input_path, output_path, bitrate_kbps)
}

fn copy_file(input_path: &str, output_path: &str) -> Result<String, String> {
    if fs::canonicalize(input_path).ok() == fs::canonicalize(output_path).ok() {
        return Ok(output_path.to_string());
    }
    if let Some(parent) = Path::new(output_path).parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建输出目录失败: {}", e))?;
    }
    fs::copy(input_path, output_path).map_err(|e| format!("复制文件失败: {}", e))?;
    Ok(output_path.to_string())
}

fn closest_bitrate(kbps: u32) -> mp3lame_encoder::Bitrate {
    use mp3lame_encoder::Bitrate::*;
    let variants: &[(u32, mp3lame_encoder::Bitrate)] = &[
        (8, Kbps8), (16, Kbps16), (24, Kbps24), (32, Kbps32),
        (40, Kbps40), (48, Kbps48), (64, Kbps64), (80, Kbps80),
        (96, Kbps96), (112, Kbps112), (128, Kbps128), (160, Kbps160),
        (192, Kbps192), (224, Kbps224), (256, Kbps256), (320, Kbps320),
    ];
    variants
        .iter()
        .min_by_key(|(v, _)| (*v as i32 - kbps as i32).abs())
        .map(|(_, b)| *b)
        .unwrap_or(Kbps192)
}

fn transcode_to_mp3(
    input_path: &str,
    output_path: &str,
    bitrate_kbps: u32,
) -> Result<String, String> {
    // 1. Probe audio format
    let src = fs::File::open(input_path).map_err(|e| format!("打开文件失败: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(src), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = Path::new(input_path).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("无法识别音频格式: {e}"))?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "未找到音频轨道".to_string())?;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("不支持的音频编码: {e}"))?;

    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let channels = track
        .codec_params
        .channels
        .map(|ch| ch.count())
        .unwrap_or(2);

    // 2. Build LAME encoder
    let num_channels = if channels >= 2 { 2u8 } else { 1u8 };
    let mut mp3_encoder = mp3lame_encoder::Builder::new()
        .ok_or_else(|| "创建 MP3 编码器失败".to_string())?
        .with_num_channels(num_channels)
        .map_err(|e| format!("编码器配置(ch): {e}"))?
        .with_sample_rate(sample_rate)
        .map_err(|e| format!("编码器配置(sr): {e}"))?
        .with_brate(closest_bitrate(bitrate_kbps))
        .map_err(|e| format!("编码器配置(br): {e}"))?
        .with_quality(mp3lame_encoder::Quality::Good)
        .map_err(|e| format!("编码器配置(ql): {e}"))?
        .build()
        .map_err(|e| format!("编码器初始化: {e}"))?;

    // Prepare output — always use a temp file for atomic write
    if let Some(parent) = Path::new(output_path).parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建输出目录失败: {}", e))?;
    }
    let tmp_output = format!("{output_path}.tmpconv");

    let mut mp3_buf: Vec<u8> = Vec::new();

    // 3. Decode + Encode loop
    let result = (|| -> Result<(), String> {
        loop {
            let packet = match format.next_packet() {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(e) => return Err(format!("读取音频: {e}")),
            };

            let decoded = decoder.decode(&packet).map_err(|e| format!("解码: {e}"))?;

            let spec = *decoded.spec();
            let duration = decoded.capacity() as u64;
            let mut sample_buf = SampleBuffer::<i16>::new(duration, spec);
            sample_buf.copy_interleaved_ref(decoded);

            let pcm_i16 = sample_buf.samples();
            if pcm_i16.is_empty() {
                continue;
            }

            encode_pcm(&mut mp3_encoder, pcm_i16, num_channels, &mut mp3_buf)?;
        }

        // Flush
        mp3_buf.reserve(7200);
        let w = mp3_encoder
            .flush::<mp3lame_encoder::FlushNoGap>(mp3_buf.spare_capacity_mut())
            .map_err(|e| format!("MP3 收尾: {e}"))?;
        unsafe { mp3_buf.set_len(mp3_buf.len() + w); }

        let mut out_file =
            fs::File::create(&tmp_output).map_err(|e| format!("创建输出文件: {e}"))?;
        out_file
            .write_all(&mp3_buf)
            .map_err(|e| format!("写入输出: {e}"))?;
        Ok(())
    })();

    // Always clean up temp file on error
    if let Err(e) = &result {
        let _ = fs::remove_file(&tmp_output);
        return Err(e.clone());
    }

    // Atomic rename: temp → final
    fs::rename(&tmp_output, output_path)
        .map_err(|e| format!("替换输出: {e}"))?;

    Ok(output_path.to_string())
}

fn encode_pcm(
    encoder: &mut mp3lame_encoder::Encoder,
    pcm_i16: &[i16],
    num_channels: u8,
    mp3_buf: &mut Vec<u8>,
) -> Result<(), String> {
    if num_channels >= 2 {
        let half = pcm_i16.len() / 2;
        let mut left = Vec::with_capacity(half + 1);
        let mut right = Vec::with_capacity(half + 1);
        for pair in pcm_i16.chunks(2) {
            left.push(pair[0]);
            right.push(if pair.len() > 1 { pair[1] } else { 0 });
        }
        mp3_buf.reserve(mp3lame_encoder::max_required_buffer_size(left.len()));
        let pcm = mp3lame_encoder::DualPcm {
            left: &left,
            right: &right,
        };
        let w = encoder
            .encode(pcm, mp3_buf.spare_capacity_mut())
            .map_err(|e| format!("MP3 编码: {e}"))?;
        unsafe { mp3_buf.set_len(mp3_buf.len() + w); }
    } else {
        mp3_buf.reserve(mp3lame_encoder::max_required_buffer_size(pcm_i16.len()));
        let pcm = mp3lame_encoder::MonoPcm(pcm_i16);
        let w = encoder
            .encode(pcm, mp3_buf.spare_capacity_mut())
            .map_err(|e| format!("MP3 编码: {e}"))?;
        unsafe { mp3_buf.set_len(mp3_buf.len() + w); }
    }
    Ok(())
}


