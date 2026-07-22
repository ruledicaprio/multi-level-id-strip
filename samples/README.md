# samples/

Identity-document specimen images used as test/example fixtures for the OCR,
LLM, and pipeline crates. Files are organized by document kind into
subdirectories; **no file has been renamed** (see "Naming convention" below
for why).

## Layout

```
samples/
  passports/          ~100 passport specimen images (TD3 MRZ format)
  id_cards/            ~25 national identity card images (TD1 / TD2 MRZ format)
  driving_licenses/     3  driving licence specimen images (no MRZ)
  ocr_fixtures/         6  docling OCR test fixtures, each an image + .md + .json triple
  misc/                 3  unclassifiable specimens (e.g. border-pass documents, wiki reference image)
  README.md
```

`tools/fetch_wiki_category_with_images.py` (repo root `tools/`) is the
scraper originally used to collect the Wikipedia specimen images; it lives
outside `samples/` since it is not itself a test fixture.

## Document kind -> MRZ format

| Directory            | Document kind              | MRZ format                                   |
|-----------------------|-----------------------------|-----------------------------------------------|
| `passports/`          | Passport                    | TD3 (2 lines x 44 chars, on the biodata page) |
| `id_cards/`           | National identity card      | TD1 (3 x 30) or TD2 (2 x 36); MRZ is often on the *back/rear* image of a front+back pair |
| `driving_licenses/`   | Driving licence              | No MRZ — not a machine-readable travel document |
| `ocr_fixtures/`       | Mixed (passport/ID pages used for docling OCR ground-truth) | Varies; see each fixture's `.md`/`.json` |

## Naming convention for FUTURE additions

Existing filenames are used as lookup keys by several tests (`find_sample`
/ `walk_samples` helpers walk `samples/` recursively and match by
**basename**), so **existing files are intentionally left un-renamed** when
moved into subdirectories — renaming any of them would break those test
references.

New files added going forward should follow:

```
Country_DocType_Detail.ext
```

- `Country` — the issuing country/territory, `Snake_Case` or `Title_Case`
  matching existing entries (e.g. `Croatia`, `North_Macedonia`).
- `DocType` — one of `Passport`, `ID_Card`, `Driving_License`, matching the
  subdirectory the file will be placed in.
- `Detail` (optional) — page/variant qualifier, e.g. `Specimen`,
  `Specimen_2`, `Biodata`, `Front`, `Back`, `Cover`.
- `ext` — prefer `jpg`/`png`; avoid `webp`/`gif` unless that is the only
  source format available.

Basenames must stay **unique across all of `samples/`**, regardless of
which subdirectory they end up in, since lookups are basename-based and do
not disambiguate by directory.

## Provenance

Images are public specimen / illustrative document images collected from
Wikipedia's "Passports by country" category and related identity-document
categories (national ID cards, driving licences). They depict specimen or
void documents published for public reference, not real issued documents
belonging to private individuals, unless explicitly marked otherwise (e.g.
filenames containing `_private`).

## Licensing / usage

These images are used here strictly for specimen/illustrative purposes —
as test fixtures for MRZ/OCR parsing and layout detection. Refer to the
original Wikipedia/Wikimedia Commons file pages for the specific license
of each image if redistribution outside this repository's test suite is
ever needed.

## Format / count summary

Total files under `samples/`: 149 (137 images + 6 `.md` + 6 `.json` OCR
fixture sidecars).

| Format | Count |
|--------|-------|
| JPG (`.jpg`)   | 85 |
| JPEG (`.jpeg`) | 3  |
| PNG (`.png`)   | 33 |
| WEBP (`.webp`) | 14 |
| GIF (`.gif`)   | 2  |
| MD (`.md`)     | 6  |
| JSON (`.json`) | 6  |

Per-directory counts: `passports/` 100, `id_cards/` 25,
`driving_licenses/` 3, `ocr_fixtures/` 18 (6 image + 6 `.md` + 6 `.json`),
`misc/` 3.
