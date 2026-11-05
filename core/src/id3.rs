//! 极简 ID3v2.3 标签写入（不引第三方库）。
//!
//! 为什么用 UTF-16 而不是 UTF-8：ID3v2.3 规范只定义 ISO-8859-1 与 UTF-16，
//! UTF-8 是 v2.4 才有的。中文标题用 ISO-8859-1 会乱码，所以写 UTF-16(BOM)。

use std::io::{Read, Seek, SeekFrom, Write};

const FRAME_TIT2: &[u8; 4] = b"TIT2";
const FRAME_TPE1: &[u8; 4] = b"TPE1";
const FRAME_TALB: &[u8; 4] = b"TALB";
const FRAME_TYER: &[u8; 4] = b"TYER";

fn syncsafe(n: u32) -> [u8; 4] {
    [
        ((n >> 21) & 0x7F) as u8,
        ((n >> 14) & 0x7F) as u8,
        ((n >> 7) & 0x7F) as u8,
        (n & 0x7F) as u8,
    ]
}

fn text_frame(id: &[u8; 4], text: &str) -> Vec<u8> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut body = Vec::with_capacity(text.len() * 2 + 3);
    body.push(0x01); // UTF-16 with BOM
    body.extend_from_slice(&[0xFF, 0xFE]); // little-endian BOM
    for unit in text.encode_utf16() {
        body.extend_from_slice(&unit.to_le_bytes());
    }
    // 结尾的 UTF-16 NUL
    body.extend_from_slice(&[0x00, 0x00]);

    let mut frame = Vec::with_capacity(10 + body.len());
    frame.extend_from_slice(id);
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes()); // v2.3: 普通大端
    frame.extend_from_slice(&[0x00, 0x00]); // flags
    frame.extend_from_slice(&body);
    frame
}

/// 构造完整 ID3v2.3 标签块
pub fn build_tag(title: &str, artist: &str, album: &str, year: &str) -> Vec<u8> {
    let mut frames = Vec::new();
    frames.extend(text_frame(FRAME_TIT2, title));
    frames.extend(text_frame(FRAME_TPE1, artist));
    frames.extend(text_frame(FRAME_TALB, album));
    frames.extend(text_frame(FRAME_TYER, year));

    let mut out = Vec::with_capacity(10 + frames.len());
    out.extend_from_slice(b"ID3");
    out.extend_from_slice(&[0x03, 0x00]); // v2.3.0
    out.push(0x00); // flags
    out.extend_from_slice(&syncsafe(frames.len() as u32));
    out.extend_from_slice(&frames);
    out
}

/// 若文件已有 ID3v2 标签，返回其总长度（用于剥离，避免叠加标签）
pub fn existing_tag_len<R: Read + Seek>(r: &mut R) -> std::io::Result<u64> {
    let mut header = [0u8; 10];
    r.seek(SeekFrom::Start(0))?;
    if r.read_exact(&mut header).is_err() {
        return Ok(0);
    }
    if &header[0..3] != b"ID3" {
        return Ok(0);
    }
    let size = ((header[6] as u64) << 21)
        | ((header[7] as u64) << 14)
        | ((header[8] as u64) << 7)
        | (header[9] as u64);
    let footer = if header[5] & 0x10 != 0 { 10 } else { 0 };
    Ok(10 + size + footer)
}

/// 给一个音频文件写入标签（先剥离旧标签）
pub fn write_tags(
    path: &std::path::Path,
    title: &str,
    artist: &str,
    album: &str,
) -> std::io::Result<()> {
    let mut src = std::fs::File::open(path)?;
    let skip = existing_tag_len(&mut src)?;

    let mut audio = Vec::new();
    src.seek(SeekFrom::Start(skip))?;
    src.read_to_end(&mut audio)?;
    drop(src);

    let tag = build_tag(title, artist, album, "");
    let tmp = path.with_extension("tagged.tmp");
    {
        let mut out = std::fs::File::create(&tmp)?;
        out.write_all(&tag)?;
        out.write_all(&audio)?;
        out.flush()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_header_is_wellformed() {
        let tag = build_tag("稻香", "周杰伦", "魔杰座", "");
        assert_eq!(&tag[0..3], b"ID3");
        assert_eq!(tag[3], 3);
        assert_eq!(tag[4], 0);
        // syncsafe 解码应与帧区长度一致
        let size = ((tag[6] as usize) << 21)
            | ((tag[7] as usize) << 14)
            | ((tag[8] as usize) << 7)
            | (tag[9] as usize);
        assert_eq!(size, tag.len() - 10);
    }

    #[test]
    fn contains_title_frame() {
        let tag = build_tag("Title", "Artist", "Album", "");
        let win = b"TIT2";
        let pos = tag.windows(4).position(|w| w == win).expect("缺 TIT2");
        // 帧长度字段（大端）
        let len = u32::from_be_bytes([tag[pos + 4], tag[pos + 5], tag[pos + 6], tag[pos + 7]]);
        assert!(len > 0);
        // 编码字节为 0x01（UTF-16）
        assert_eq!(tag[pos + 10], 0x01);
    }

    #[test]
    fn roundtrip_through_file() {
        let dir = std::env::temp_dir().join("tinymusic_id3_test");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("a.mp3");
        std::fs::write(&p, b"FAKEAUDIO0000").unwrap();
        write_tags(&p, "标题", "艺人", "专辑").unwrap();
        let data = std::fs::read(&p).unwrap();
        assert_eq!(&data[0..3], b"ID3");
        assert!(data.ends_with(b"FAKEAUDIO0000"));

        // 再写一次不应叠加标签
        write_tags(&p, "标题2", "艺人", "专辑").unwrap();
        let data2 = std::fs::read(&p).unwrap();
        // 只应有一个 ID3 头，且音频段仍在尾部
        assert_eq!(&data2[0..3], b"ID3");
        assert!(data2.ends_with(b"FAKEAUDIO0000"));
        let mut f = std::fs::File::open(&p).unwrap();
        let skip = existing_tag_len(&mut f).unwrap();
        assert!(skip as usize > 10);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
