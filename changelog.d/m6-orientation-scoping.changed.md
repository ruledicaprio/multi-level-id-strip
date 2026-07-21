- **Roadmap M6 scoping notes.** Two additions to [`docs/ROADMAP.md`](docs/ROADMAP.md): a design
  note on replacing the brute-force orientation probe (a full OCR detection pass at each of
  0°/90°/180°/270°, plus a separate small-tilt sweep in `preprocess::deskew`) with a single-pass
  width-weighted circular mean over the per-word angles `ocrs` already reports and `geometry.rs`
  currently discards — including why the angle must be doubled before averaging, why it can never
  resolve 0° vs 180°, and why an FFT-based approach buys nothing here; and a note that driving
  licences are a *different mechanism* rather than a lower-priority document class, since they
  carry no MRZ at all (AAMVA PDF417 in the US, nothing in the EU) and belong to the declared
  `ExtractionV2.barcodes` slot rather than the MRZ roadmap.
