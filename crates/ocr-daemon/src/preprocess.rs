//! Image preprocessing for OCR: DPI normalization, orientation correction,
//! deskew, and grayscale + Otsu global binarization.
//!
//! High-contrast black-text-on-white markedly improves Tesseract accuracy on
//! low-contrast document photos. Otsu picks the threshold that maximizes
//! between-class variance of the intensity histogram — parameter-free and
//! deterministic. The other three steps correct the common failure modes of
//! phone-camera document photos: too low a resolution, rotated 90/180/270
//! degrees, or skewed by a few degrees.

use image::imageops::FilterType;
use image::{DynamicImage, GrayImage, Luma, Rgb};
use imageproc::geometric_transformations::{rotate_about_center, Interpolation};

/// Below this on the shorter side, Tesseract accuracy degrades sharply — the
/// standard "aim for ~300 DPI-equivalent" heuristic, applied without trusting
/// (often missing/wrong) EXIF DPI tags.
const MIN_DIM: u32 = 1500;

/// Upscale small images so their shorter side reaches [`MIN_DIM`], preserving
/// aspect ratio. A no-op if the image is already large enough.
pub fn normalize_dpi(img: &DynamicImage) -> DynamicImage {
    let (w, h) = (img.width(), img.height());
    let shorter = w.min(h);
    if shorter == 0 || shorter >= MIN_DIM {
        return img.clone();
    }
    let scale = f64::from(MIN_DIM) / f64::from(shorter);
    let new_w = (f64::from(w) * scale).round() as u32;
    let new_h = (f64::from(h) * scale).round() as u32;
    img.resize_exact(new_w, new_h, FilterType::Lanczos3)
}

/// Detects which of 0/90/180/270 degrees is upright by running Tesseract on
/// each candidate and keeping whichever scores the highest mean confidence
/// (`leptess::LepTess::mean_text_conf`), then returns the image rotated
/// accordingly. Falls back to the original orientation if every candidate
/// fails to produce a confident read (e.g. a blank or unreadable image).
pub fn correct_orientation(img: &DynamicImage, lang: &str) -> DynamicImage {
    let rgb = img.to_rgb8();
    let variants: [DynamicImage; 4] = [
        DynamicImage::ImageRgb8(rgb.clone()),
        DynamicImage::ImageRgb8(image::imageops::rotate90(&rgb)),
        DynamicImage::ImageRgb8(image::imageops::rotate180(&rgb)),
        DynamicImage::ImageRgb8(image::imageops::rotate270(&rgb)),
    ];

    let mut best = 0usize;
    let mut best_conf = -1i32;
    for (i, variant) in variants.iter().enumerate() {
        let conf = orientation_confidence(variant, lang);
        if conf > best_conf {
            best_conf = conf;
            best = i;
        }
    }
    variants.into_iter().nth(best).expect("index in bounds")
}

/// Mean Tesseract confidence (0-100, or -1 on any failure) for `img` as-is —
/// used to score orientation candidates in [`correct_orientation`].
fn orientation_confidence(img: &DynamicImage, lang: &str) -> i32 {
    let bin = binarize(img);
    let mut buf = std::io::Cursor::new(Vec::new());
    if bin.write_to(&mut buf, image::ImageFormat::Png).is_err() {
        return -1;
    }
    let Ok(mut lt) = leptess::LepTess::new(None, lang) else {
        return -1;
    };
    if lt.set_image_from_mem(buf.get_ref()).is_err() {
        return -1;
    }
    // Populates the confidence stats read by `mean_text_conf` below.
    let _ = lt.get_utf8_text();
    lt.mean_text_conf()
}

/// Search range for skew correction. Orientation is corrected first
/// ([`correct_orientation`]), so only small-angle skew remains — a wider
/// range risks confusing skew with orientation.
const DESKEW_RANGE_DEG: f32 = 15.0;
const DESKEW_STEP_DEG: f32 = 0.5;
/// Cap the working copy's longest side during angle search: skew angle is
/// scale-invariant, and searching at full resolution is needlessly slow.
const DESKEW_SEARCH_MAX_DIM: u32 = 800;

/// Corrects small-angle skew via the projection-profile method: binarizes a
/// working copy, searches candidate rotation angles for the one that
/// maximizes the variance of per-row black-pixel counts (text lines produce a
/// high-variance histogram when horizontal, a flat one when skewed), then
/// rotates the original image by that angle.
pub fn deskew(img: &DynamicImage) -> DynamicImage {
    let bin_full = binarize(img).to_luma8();
    let (w, h) = bin_full.dimensions();
    let longest = w.max(h);
    let search_bin = if longest > DESKEW_SEARCH_MAX_DIM {
        let scale = f64::from(DESKEW_SEARCH_MAX_DIM) / f64::from(longest);
        image::imageops::resize(
            &bin_full,
            (f64::from(w) * scale).round() as u32,
            (f64::from(h) * scale).round() as u32,
            FilterType::Nearest,
        )
    } else {
        bin_full
    };

    let mut best_angle_deg = 0.0f32;
    let mut best_var = -1.0f64;
    let mut angle_deg = -DESKEW_RANGE_DEG;
    while angle_deg <= DESKEW_RANGE_DEG {
        let theta = angle_deg.to_radians();
        let rotated =
            rotate_about_center(&search_bin, theta, Interpolation::Nearest, Luma([255u8]));
        let var = row_projection_variance(&rotated);
        if var > best_var {
            best_var = var;
            best_angle_deg = angle_deg;
        }
        angle_deg += DESKEW_STEP_DEG;
    }

    if best_angle_deg == 0.0 {
        return img.clone();
    }
    let rgb = img.to_rgb8();
    let rotated = rotate_about_center(
        &rgb,
        best_angle_deg.to_radians(),
        Interpolation::Bilinear,
        Rgb([255u8, 255, 255]),
    );
    DynamicImage::ImageRgb8(rotated)
}

/// Variance of per-row black-pixel counts — high when text lines are
/// horizontal (alternating dense/sparse rows), low when skewed (rows blend
/// together).
fn row_projection_variance(gray: &GrayImage) -> f64 {
    let (w, h) = gray.dimensions();
    if h == 0 {
        return 0.0;
    }
    let counts: Vec<u32> = (0..h)
        .map(|y| (0..w).filter(|&x| gray.get_pixel(x, y)[0] == 0).count() as u32)
        .collect();
    let mean = counts.iter().map(|&c| f64::from(c)).sum::<f64>() / f64::from(h);
    counts
        .iter()
        .map(|&c| (f64::from(c) - mean).powi(2))
        .sum::<f64>()
        / f64::from(h)
}

/// Convert to grayscale and binarize with Otsu's method.
pub fn binarize(img: &DynamicImage) -> DynamicImage {
    let gray: GrayImage = img.to_luma8();
    let t = otsu_threshold(&gray) as u16;
    let out = GrayImage::from_fn(gray.width(), gray.height(), |x, y| {
        if u16::from(gray.get_pixel(x, y)[0]) > t {
            Luma([255])
        } else {
            Luma([0])
        }
    });
    DynamicImage::ImageLuma8(out)
}

/// Otsu's global threshold from the 256-bin intensity histogram.
pub fn otsu_threshold(gray: &GrayImage) -> u8 {
    let mut hist = [0u32; 256];
    for p in gray.pixels() {
        hist[p[0] as usize] += 1;
    }
    let total = gray.width() * gray.height();
    if total == 0 {
        return 127;
    }
    let sum: f64 = hist
        .iter()
        .enumerate()
        .map(|(i, &c)| i as f64 * c as f64)
        .sum();

    let mut sum_b = 0f64;
    let mut w_b = 0u32;
    let mut max_var = -1f64;
    let mut thresh = 0u8;
    for (t, &count) in hist.iter().enumerate() {
        w_b += count;
        if w_b == 0 {
            continue;
        }
        let w_f = total - w_b;
        if w_f == 0 {
            break;
        }
        sum_b += t as f64 * count as f64;
        let m_b = sum_b / w_b as f64;
        let m_f = (sum - sum_b) / w_f as f64;
        let var = w_b as f64 * w_f as f64 * (m_b - m_f).powi(2);
        if var > max_var {
            max_var = var;
            thresh = t as u8;
        }
    }
    thresh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn otsu_separates_two_clusters() {
        // Dark cluster at 60, light cluster at 200. Otsu returns the threshold
        // `t` such that `pixel > t` cleanly separates them: t sits at/above the
        // dark value and strictly below the light value.
        let img = GrayImage::from_fn(10, 10, |x, _| Luma([if x < 5 { 60 } else { 200 }]));
        let t = otsu_threshold(&img);
        assert!(
            (60..200).contains(&t),
            "threshold {t} does not separate 60|200"
        );
    }

    #[test]
    fn binarize_is_pure_black_and_white() {
        let img = DynamicImage::ImageLuma8(GrayImage::from_fn(8, 8, |x, _| {
            Luma([if x < 4 { 40 } else { 200 }])
        }));
        let out = binarize(&img).to_luma8();
        for p in out.pixels() {
            assert!(
                p[0] == 0 || p[0] == 255,
                "binarized pixel not 0/255: {}",
                p[0]
            );
        }
    }

    #[test]
    fn normalize_dpi_upscales_small_images() {
        let img = DynamicImage::ImageLuma8(GrayImage::from_pixel(100, 50, Luma([128])));
        let out = normalize_dpi(&img);
        assert_eq!(
            out.width(),
            MIN_DIM * 2,
            "shorter side (height) hits MIN_DIM"
        );
        assert_eq!(out.height(), MIN_DIM);
    }

    #[test]
    fn normalize_dpi_leaves_large_images_unchanged() {
        let img = DynamicImage::ImageLuma8(GrayImage::from_pixel(2000, 1800, Luma([128])));
        let out = normalize_dpi(&img);
        assert_eq!((out.width(), out.height()), (2000, 1800));
    }

    /// Builds a synthetic "document": horizontal stripes of black text-lines
    /// on a white background, so its skew angle is unambiguous.
    fn striped_test_image(w: u32, h: u32) -> GrayImage {
        GrayImage::from_fn(w, h, |_, y| {
            if (y / 6) % 2 == 0 {
                Luma([0])
            } else {
                Luma([255])
            }
        })
    }

    #[test]
    fn deskew_undoes_a_known_rotation() {
        let stripes = striped_test_image(200, 200);
        let upright_variance = row_projection_variance(&stripes);

        let skewed = rotate_about_center(
            &stripes,
            5.0f32.to_radians(),
            Interpolation::Bilinear,
            Luma([255u8]),
        );
        let skewed_variance = row_projection_variance(&skewed);
        assert!(
            skewed_variance < upright_variance,
            "sanity check: rotating should reduce row-projection variance"
        );

        let corrected = deskew(&DynamicImage::ImageLuma8(skewed)).to_luma8();
        let corrected_variance = row_projection_variance(&corrected);
        assert!(
            corrected_variance > skewed_variance,
            "deskew should recover most of the lost row-projection variance \
             (skewed: {skewed_variance}, corrected: {corrected_variance}, upright: {upright_variance})"
        );
    }
}
