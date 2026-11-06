# Makefile — build targets for the Agentical paper
#
# Prerequisites (any one suffices):
#   Path A — tectonic:  brew install tectonic
#   Path B — MacTeX:    brew install --cask mactex  (or mactex-no-gui)
#             then:     latexmk is at /Library/TeX/texbin/latexmk
#   Path C — Overleaf:  upload the paper/ directory; no local tools needed.
#
# All targets assume you run `make` from the paper/ directory.

MAIN    = main
TEXSRC  = $(MAIN).tex refs.bib \
          $(wildcard sections/*.tex) \
          $(wildcard figures/*.tex) $(wildcard figures/*.pdf)

.PHONY: all pdf tectonic watch clean distclean help

# ---- Default target --------------------------------------------------
all: pdf

# ---- Path B: pdflatex via latexmk ------------------------------------
# Runs pdflatex + bibtex as many times as needed.
# latexmkrc in this directory provides the engine/flag defaults.
pdf: $(MAIN).pdf

$(MAIN).pdf: $(TEXSRC)
	latexmk -pdf $(MAIN).tex

# ---- Path A: tectonic (single binary, downloads packages on-demand) --
# Tectonic handles all reruns automatically.
tectonic: $(TEXSRC)
	tectonic $(MAIN).tex

# ---- Continuous preview (Path B only) --------------------------------
watch:
	latexmk -pdf -pvc $(MAIN).tex

# ---- Clean -----------------------------------------------------------
clean:
	latexmk -c
	rm -f $(MAIN).bbl $(MAIN).blg

distclean:
	latexmk -C
	rm -f $(MAIN).bbl $(MAIN).blg $(MAIN).pdf

# ---- Help ------------------------------------------------------------
help:
	@echo "Targets:"
	@echo "  make pdf        — build $(MAIN).pdf with pdflatex + latexmk (Path B)"
	@echo "  make tectonic   — build $(MAIN).pdf with tectonic (Path A)"
	@echo "  make watch      — continuous rebuild on save (latexmk -pvc)"
	@echo "  make clean      — remove intermediate files"
	@echo "  make distclean  — remove all generated files including $(MAIN).pdf"
	@echo ""
	@echo "Conference-class switch (edit $(MAIN).tex \\documentclass line):"
	@echo "  POPL/OOPSLA:  \\documentclass[acmsmall,review]{acmart}"
	@echo "  LICS:         \\documentclass[a4paper]{easychair}"
	@echo "  arXiv:        \\documentclass[11pt]{article}   (current — no change)"
	@echo ""
	@echo "See BUILD.md for full instructions."
