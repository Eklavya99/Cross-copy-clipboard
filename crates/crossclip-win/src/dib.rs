//! Conversion between Windows device-independent bitmaps (`CF_DIB` / `CF_DIBV5`)
//! and PNG, the format images use on the wire.
//!
//! A clipboard DIB is a `BITMAPINFOHEADER` (or the larger V4/V5 variants), then
//! optional colour masks and palette, then the pixel rows. There is no
//! `BITMAPFILEHEADER`.

use std::io::Cursor;

use anyhow::{bail, ensure, Context, Result};

const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;
const BI_ALPHABITFIELDS: u32 = 6;
const BITMAPINFOHEADER_SIZE: usize = 40;
const BITMAPV5HEADER_SIZE: usize = 124;
/// `LCS_sRGB` colour space tag ('sRGB').
const LCS_SRGB: u32 = 0x7352_4742;
const LCS_GM_IMAGES: u32 = 4;
/// Refuse absurd dimensions from malformed headers before allocating.
const MAX_DIMENSION: u32 = 32_768;

/// An 8-bit RGBA image, row-major, top row first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Decodes a clipboard DIB (`CF_DIB` or `CF_DIBV5` payload).
pub fn decode_dib(dib: &[u8]) -> Result<Rgba> {
    let header_size = read_u32(dib, 0)? as usize;
    ensure!(
        header_size >= BITMAPINFOHEADER_SIZE,
        "unsupported DIB header size {header_size}"
    );
    let width = read_i32(dib, 4)?;
    let raw_height = read_i32(dib, 8)?;
    let bit_count = read_u16(dib, 14)?;
    let compression = read_u32(dib, 16)?;
    let colors_used = read_u32(dib, 32)?;

    ensure!(width > 0 && raw_height != 0, "empty DIB");
    let width = width as u32;
    let height = raw_height.unsigned_abs();
    ensure!(
        width <= MAX_DIMENSION && height <= MAX_DIMENSION,
        "DIB too large"
    );
    let bottom_up = raw_height > 0;

    // Colour masks live inside V3+ headers, or right after a plain 40-byte header.
    let mut offset = header_size;
    let masks = match compression {
        BI_RGB => match bit_count {
            16 => Masks::from_rgba(0x7C00, 0x03E0, 0x001F, 0),
            32 => Masks::from_rgba(0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000),
            _ => Masks::default(),
        },
        BI_BITFIELDS | BI_ALPHABITFIELDS => {
            let (base, count) = if header_size == BITMAPINFOHEADER_SIZE {
                let count = if compression == BI_ALPHABITFIELDS {
                    4
                } else {
                    3
                };
                offset += count * 4;
                (BITMAPINFOHEADER_SIZE, count)
            } else {
                // V2 has RGB masks, V3 and later also alpha.
                (BITMAPINFOHEADER_SIZE, if header_size >= 56 { 4 } else { 3 })
            };
            let alpha = if count == 4 {
                read_u32(dib, base + 12)?
            } else {
                0
            };
            Masks::from_rgba(
                read_u32(dib, base)?,
                read_u32(dib, base + 4)?,
                read_u32(dib, base + 8)?,
                alpha,
            )
        }
        other => bail!("unsupported DIB compression {other}"),
    };

    let palette = if bit_count <= 8 {
        let entries = if colors_used == 0 {
            1usize << bit_count
        } else {
            colors_used as usize
        };
        let bytes = dib
            .get(offset..offset + entries * 4)
            .context("DIB palette truncated")?;
        offset += entries * 4;
        bytes
            .chunks_exact(4)
            .map(|c| [c[2], c[1], c[0], 255])
            .collect()
    } else {
        Vec::new()
    };

    let stride = (width as usize * bit_count as usize).div_ceil(32) * 4;
    let data = dib
        .get(offset..offset + stride * height as usize)
        .context("DIB pixel data truncated")?;

    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height as usize {
        let src_row = if bottom_up {
            height as usize - 1 - y
        } else {
            y
        };
        let row = &data[src_row * stride..(src_row + 1) * stride];
        for x in 0..width as usize {
            let px = match bit_count {
                1 | 4 | 8 => {
                    let bits = bit_count as usize;
                    let bit = x * bits;
                    let shift = 8 - bits - bit % 8;
                    let index = (row[bit / 8] >> shift) as usize & ((1 << bits) - 1);
                    *palette
                        .get(index)
                        .context("DIB palette index out of range")?
                }
                16 => masks.apply(u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]) as u32),
                24 => [row[x * 3 + 2], row[x * 3 + 1], row[x * 3], 255],
                32 => {
                    let v = &row[x * 4..x * 4 + 4];
                    masks.apply(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
                }
                other => bail!("unsupported DIB bit depth {other}"),
            };
            pixels.extend_from_slice(&px);
        }
    }

    // Many apps write 32-bit DIBs whose "alpha" byte is just padding (all zero).
    // Treat such images as fully opaque instead of fully transparent.
    if bit_count == 32 && pixels.chunks_exact(4).all(|p| p[3] == 0) {
        pixels.chunks_exact_mut(4).for_each(|p| p[3] = 255);
    }

    Ok(Rgba {
        width,
        height,
        pixels,
    })
}

/// Encodes an image as a `CF_DIBV5` payload: 32-bit BGRA, bottom-up, with alpha.
pub fn encode_dibv5(image: &Rgba) -> Vec<u8> {
    let pixel_bytes = image.width as usize * image.height as usize * 4;
    let mut out = Vec::with_capacity(BITMAPV5HEADER_SIZE + pixel_bytes);
    let u32s = |out: &mut Vec<u8>, values: &[u32]| {
        values
            .iter()
            .for_each(|v| out.extend_from_slice(&v.to_le_bytes()))
    };

    u32s(
        &mut out,
        &[BITMAPV5HEADER_SIZE as u32, image.width, image.height],
    );
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bit count
    u32s(
        &mut out,
        &[BI_BITFIELDS, pixel_bytes as u32, 2835, 2835, 0, 0],
    );
    u32s(
        &mut out,
        &[0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000, LCS_SRGB],
    );
    out.resize(out.len() + 36 + 12, 0); // CIEXYZTRIPLE endpoints + gamma
    u32s(&mut out, &[LCS_GM_IMAGES, 0, 0, 0]); // intent, profile data/size, reserved
    debug_assert_eq!(out.len(), BITMAPV5HEADER_SIZE);

    let row_bytes = image.width as usize * 4;
    for row in image.pixels.chunks_exact(row_bytes).rev() {
        for p in row.chunks_exact(4) {
            out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    out
}

/// Encodes RGBA pixels as PNG.
pub fn encode_png(image: &Rgba) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&image.pixels)?;
    writer.finish()?;
    Ok(out)
}

/// Decodes any PNG into 8-bit RGBA.
pub fn decode_png(data: &[u8]) -> Result<Rgba> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let (width, height) = reader.info().size();
    ensure!(
        width <= MAX_DIMENSION && height <= MAX_DIMENSION,
        "PNG too large"
    );
    let mut buf = vec![0; reader.output_buffer_size().context("PNG too large")?];
    let frame = reader.next_frame(&mut buf)?;
    buf.truncate(frame.buffer_size());

    let pixels = match frame.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => bail!("indexed PNG was not expanded"),
    };
    Ok(Rgba {
        width,
        height,
        pixels,
    })
}

/// Reads only the dimensions from a PNG header.
pub fn png_dimensions(data: &[u8]) -> Result<(u32, u32)> {
    let reader = png::Decoder::new(Cursor::new(data)).read_info()?;
    Ok(reader.info().size())
}

#[derive(Default, Clone, Copy)]
struct Masks([(u32, u32, u64); 4]); // (mask, shift, max value) for R, G, B, A

impl Masks {
    fn from_rgba(r: u32, g: u32, b: u32, a: u32) -> Self {
        let channel = |mask: u32| {
            if mask == 0 {
                (0, 0, 0)
            } else {
                let shift = mask.trailing_zeros();
                (mask, shift, u64::from(mask >> shift))
            }
        };
        Masks([channel(r), channel(g), channel(b), channel(a)])
    }

    fn apply(&self, value: u32) -> [u8; 4] {
        let mut out = [0, 0, 0, 255];
        for (i, &(mask, shift, max)) in self.0.iter().enumerate() {
            if mask != 0 {
                let v = u64::from((value & mask) >> shift);
                out[i] = (v * 255 / max) as u8;
            }
        }
        out
    }
}

fn read_u16(buf: &[u8], at: usize) -> Result<u16> {
    let b = buf.get(at..at + 2).context("DIB header truncated")?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(buf: &[u8], at: usize) -> Result<u32> {
    let b = buf.get(at..at + 4).context("DIB header truncated")?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_i32(buf: &[u8], at: usize) -> Result<i32> {
    read_u32(buf, at).map(|v| v as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 3×2 image with distinct colours and partial transparency.
    fn sample() -> Rgba {
        #[rustfmt::skip]
        let pixels = vec![
            255, 0, 0, 255,    0, 255, 0, 128,   0, 0, 255, 0,
            10, 20, 30, 255,   40, 50, 60, 200,  70, 80, 90, 255,
        ];
        Rgba {
            width: 3,
            height: 2,
            pixels,
        }
    }

    /// Builds a plain BITMAPINFOHEADER DIB, as older apps put on the clipboard.
    fn info_header_dib(image: &Rgba, bit_count: u16, top_down: bool, alpha: bool) -> Vec<u8> {
        let height = if top_down {
            -(image.height as i32)
        } else {
            image.height as i32
        };
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&(image.width as i32).to_le_bytes());
        dib.extend_from_slice(&height.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&bit_count.to_le_bytes());
        dib.resize(40, 0); // BI_RGB and zeroed remainder
        let stride = (image.width as usize * bit_count as usize).div_ceil(32) * 4;
        let mut rows: Vec<&[u8]> = image
            .pixels
            .chunks_exact(image.width as usize * 4)
            .collect();
        if !top_down {
            rows.reverse();
        }
        for row in rows {
            let start = dib.len();
            for p in row.chunks_exact(4) {
                dib.extend_from_slice(&[p[2], p[1], p[0]]);
                if bit_count == 32 {
                    dib.push(if alpha { p[3] } else { 0 });
                }
            }
            dib.resize(start + stride, 0);
        }
        dib
    }

    fn opaque(image: &Rgba) -> Rgba {
        let mut image = image.clone();
        image.pixels.chunks_exact_mut(4).for_each(|p| p[3] = 255);
        image
    }

    #[test]
    fn dibv5_round_trip_preserves_alpha() {
        let image = sample();
        let dib = encode_dibv5(&image);
        assert_eq!(dib.len(), 124 + 3 * 2 * 4);
        assert_eq!(decode_dib(&dib).unwrap(), image);
    }

    #[test]
    fn decodes_24_bit_bottom_up_with_row_padding() {
        let image = opaque(&sample());
        let dib = info_header_dib(&image, 24, false, false);
        assert_eq!(decode_dib(&dib).unwrap(), image);
    }

    #[test]
    fn decodes_32_bit_top_down() {
        let image = sample();
        let dib = info_header_dib(&image, 32, true, true);
        assert_eq!(decode_dib(&dib).unwrap(), image);
    }

    #[test]
    fn zero_alpha_padding_means_opaque() {
        let image = sample();
        let dib = info_header_dib(&image, 32, false, false);
        assert_eq!(decode_dib(&dib).unwrap(), opaque(&image));
    }

    #[test]
    fn decodes_8_bit_palette() {
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&2i32.to_le_bytes());
        dib.extend_from_slice(&1i32.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&8u16.to_le_bytes());
        dib.resize(32, 0);
        dib.extend_from_slice(&2u32.to_le_bytes()); // colours used
        dib.resize(40, 0);
        dib.extend_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]); // red, blue (BGRX)
        dib.extend_from_slice(&[1, 0, 0, 0]); // pixels: blue, red + padding
        let decoded = decode_dib(&dib).unwrap();
        assert_eq!(decoded.pixels, vec![0, 0, 255, 255, 255, 0, 0, 255]);
    }

    #[test]
    fn rejects_truncated_input() {
        let dib = encode_dibv5(&sample());
        assert!(decode_dib(&dib[..dib.len() - 1]).is_err());
        assert!(decode_dib(&dib[..20]).is_err());
        assert!(decode_dib(&[]).is_err());
    }

    #[test]
    fn png_round_trip() {
        let image = sample();
        let png = encode_png(&image).unwrap();
        assert_eq!(png_dimensions(&png).unwrap(), (3, 2));
        assert_eq!(decode_png(&png).unwrap(), image);
    }

    #[test]
    fn png_to_dib_and_back() {
        let image = sample();
        let png = encode_png(&image).unwrap();
        let dib = encode_dibv5(&decode_png(&png).unwrap());
        assert_eq!(encode_png(&decode_dib(&dib).unwrap()).unwrap(), png);
    }
}
