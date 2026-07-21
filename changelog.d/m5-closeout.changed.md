- **`docs/ROADMAP.md` now records M5 as complete.** It had still listed the bounded job queue /
  parallel OCR / batch API as an outstanding Atlas DoD after PR #49 shipped it, and geometry
  detection as outstanding after #48/#50. The replacement note also records the orientation
  result honestly — 41 of 42 `samples/` documents re-oriented correctly with zero false flips,
  one honest miss — and, more usefully, records why the first design (decide 0°/180° from where
  the MRZ band sits on the page) was circular and had to be replaced by a comparison between the
  two orientations. The M6 orientation scoping note is corrected to match, since it described the
  tie-break as the position test that did not survive contact with the corpus.
