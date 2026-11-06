# BUILD.md — How to compile the Agentical paper

## TL;DR

Fastest path (no LaTeX install needed):

```bash
brew install tectonic
cd paper
tectonic main.tex        # → main.pdf in the current directory
```

---

## Three build paths

### Path A — Tectonic (simplest, recommended for first-time build)

Tectonic is a self-contained LaTeX engine that downloads missing packages
automatically on first run (internet required only for that first run;
subsequent runs are fully offline).

```bash
# Install (macOS)
brew install tectonic

# Build
cd paper
tectonic main.tex
# Output: main.pdf
```

No BibTeX pass is needed — tectonic handles reruns internally.

---

### Path B — MacTeX + latexmk (TeX Live toolchain)

```bash
# Install full TeX Live (macOS) — ~4 GB
brew install --cask mactex

# Reload PATH so latexmk is found
eval "$(/usr/libexec/path_helper)"

# Build
cd paper
make pdf              # runs latexmk -pdf main.tex
# OR directly:
latexmk -pdf main.tex
```

`latexmkrc` in this directory sets `$pdf_mode = 1` (pdflatex) and
`$bibtex_use = 1` (BibTeX for natbib).

For a lightweight install (~1 GB, no GUI):

```bash
brew install --cask mactex-no-gui
```

---

### Path C — Overleaf (no local install)

1. Zip the entire `paper/` directory:
   ```bash
   cd agentical   # the repository root
   zip -r agentical-paper.zip paper/
   ```
2. Go to <https://overleaf.com> → New Project → Upload Project → select
   `agentical-paper.zip`.
3. Overleaf will detect `main.tex` as the root document automatically.
4. Set the compiler to **pdfLaTeX** (default) and click Recompile.

---

## Switching to a conference document class

Edit the `\documentclass` line in `main.tex`:

| Target venue          | Class line |
|-----------------------|-----------|
| arXiv preprint (now)  | `\documentclass[11pt]{article}` |
| POPL / OOPSLA / PLDI  | `\documentclass[acmsmall,review,anonymous]{acmart}` |
| LICS                  | `\documentclass[a4paper]{easychair}` |
| LIPIcs (CONCUR etc.)  | `\documentclass[a4paper,UKenglish]{lipics-v2021}` |
| LLNCS (ESOP etc.)     | `\documentclass{llncs}` |

After changing the class you will need to:
- Add the class-specific `\acmDOI`, `\keywords`, etc. metadata commands.
- Replace `\bibliographystyle{plainnat}` with whatever the class requires
  (acmart uses `\bibliographystyle{ACM-Reference-Format}`; LIPIcs uses
  `\bibliographystyle{plainurl}`).
- Check that `geometry` is not re-applied (acmart sets its own margins).

---
