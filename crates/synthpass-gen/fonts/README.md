# Fonts for `synthpass-gen`

Two OFL (SIL Open Font License)-licensed TrueType fonts are vendored here, for rendering real
glyphs (instead of placeholder bars) on the VIZ fields and the MRZ band:

- **`ocr-b.ttf`** — an OCR-B-style monospaced font for the MRZ band. Sourced from
  [jaycee723/ocr-b](https://github.com/jaycee723/ocr-b) (`dist/OCR-B.ttf`), © 2019 Raisty,
  Reserved Font Name "OCR-B", SIL OFL 1.1. Full license text: [`OFL-ocr-b.txt`](OFL-ocr-b.txt).
- **`sans.ttf`** — PT Sans Regular, a proportional sans-serif font for the human-readable VIZ
  fields. Sourced from Google Fonts' canonical repository,
  [google/fonts `ofl/ptsans`](https://github.com/google/fonts/tree/main/ofl/ptsans)
  (`PT_Sans-Web-Regular.ttf`), © 2010 ParaType Ltd., Reserved Font Names "PT Sans"/"ParaType",
  SIL OFL 1.1. Full license text: [`OFL-sans.txt`](OFL-sans.txt).

Both are static (non-variable) TrueType files, chosen deliberately over variable-font releases
of the same families since `ab_glyph` (the rasterizer used in `src/fonts.rs`) targets classic
outline fonts, not `fvar` variation axes.

Build with the `embedded-fonts` feature to bake them into the binary via `include_bytes!`:

```sh
cargo build -p synthpass-gen --features embedded-fonts
```

Without this feature (the default), `fonts::load_fonts` returns `FontError::NotEmbedded` and the
renderer draws placeholder bars in the exact layout rectangles instead — bounding boxes in
`Labels` stay meaningful either way. The unconditional "SYNTHETIC / SPECIMEN" watermark and the
generic, non-country template render regardless of this feature; they do not depend on any TTF.

Both fonts' OFL licenses are also summarized in the root [`THIRD_PARTY_NOTICES.md`](../../../THIRD_PARTY_NOTICES.md).
