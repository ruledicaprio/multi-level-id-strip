//! Image preprocessing for OCR: grayscale + Otsu global binarization.
//!
//! High-contrast black-text-on-white markedly improves Tesseract accuracy on
//! low-contrast document photos. Otsu picks the threshold that maximizes
//! between-class variance of the intensity histogram — parameter-free and
//! deterministic.

use image::{DynamicImage, GrayImage, Luma};

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
}
