# Third-party notices

Nexterm itself is dual-licensed under [MIT](LICENSE-MIT) and
[Apache-2.0](LICENSE-APACHE). This file covers third-party material **vendored
into this repository and redistributed inside the built binaries**.

Cargo dependencies are not listed here. Their licences are resolved from
`Cargo.lock` and checked by `cargo deny` against the policy in `deny.toml`. A
vendored asset such as the icon font below is *not* a Cargo dependency and
therefore not covered by that check — this file is the only control over it, so
add an entry here whenever a non-Cargo asset is vendored.

---

## Fluent System Icons

- **Upstream:** <https://github.com/microsoft/fluentui-system-icons>
- **Pinned revision:** `fb047fb395f45ccf1129f8eaee672c9dfa99152e` (2026-08-21)
- **Source file:** `fonts/FluentSystemIcons-Regular.ttf`
  (2,818,416 bytes, sha256
  `9c55ac8e041aa905d2a09d4a7e57a156dece1df99cd64952467348da0e158db4`)
- **Vendored as:** `assets/fonts/NextermIcons-Regular.ttf`
- **Licence:** MIT (full text below)

### Why a commit rather than a tag

Upstream tags are per-npm-package (for example
`react-icons-svg-sprite-subsetting-webpack-plugin@0.0.6`) and say nothing about
the state of the font files. The pin is therefore the last commit that touched
`fonts/FluentSystemIcons-Regular.ttf`, and `scripts/subset-icon-font.sh`
verifies the sha256 above on every regeneration.

### What was modified

The vendored file is a **subset** of the upstream font, not a copy of it. The
upstream face carries 9,708 icons; Nexterm draws the 19 listed in
`assets/fonts/icon-set.txt`, so the subset is cut with `fonttools`' `pyftsubset`
to the codepoints those names resolve to. The name table is rewritten so the
family reads `Nexterm Icons`, which is how the client resolves it and what keeps
it from being confused with a user-installed copy of Fluent System Icons.
Outlines are unmodified. `scripts/subset-icon-font.sh` reproduces the vendored
file byte for byte from the pinned upstream revision.

### Licence text

```
MIT License

Copyright (c) 2020 Microsoft Corporation

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
