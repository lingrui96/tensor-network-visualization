.PHONY: preview

PREVIEW_DIR := .preview

preview:
	mkdir -p $(PREVIEW_DIR)
	TEXINPUTS=tex/prototype: lualatex -interaction=nonstopmode -halt-on-error -output-directory=$(PREVIEW_DIR) examples/effects.tex
	TEXINPUTS=tex/prototype: lualatex -interaction=nonstopmode -halt-on-error -output-directory=$(PREVIEW_DIR) examples/bonds.tex
	pdftoppm -png -r 220 $(PREVIEW_DIR)/effects.pdf $(PREVIEW_DIR)/effect
	mv $(PREVIEW_DIR)/effect-1.png $(PREVIEW_DIR)/orb.png
	mv $(PREVIEW_DIR)/effect-2.png $(PREVIEW_DIR)/rounded-rectangle.png
	mv $(PREVIEW_DIR)/effect-3.png $(PREVIEW_DIR)/triangle.png
	mv $(PREVIEW_DIR)/effect-4.png $(PREVIEW_DIR)/diamond.png
	pdftoppm -png -r 220 $(PREVIEW_DIR)/bonds.pdf $(PREVIEW_DIR)/bond
	mv $(PREVIEW_DIR)/bond-1.png $(PREVIEW_DIR)/bonds-line.png
	mv $(PREVIEW_DIR)/bond-2.png $(PREVIEW_DIR)/bonds-straight-tube.png
	mv $(PREVIEW_DIR)/bond-3.png $(PREVIEW_DIR)/bonds-polyline-tube.png
	mv $(PREVIEW_DIR)/bond-4.png $(PREVIEW_DIR)/bonds-crossings.png
	mv $(PREVIEW_DIR)/bond-5.png $(PREVIEW_DIR)/bonds-labels.png

.PHONY: geometry-preview

# Plain debugging pictures of the engine's layout and geometry.
geometry-preview:
	mkdir -p $(PREVIEW_DIR)
	cargo build -q -p tnviz-cli
	for f in examples/tnv/*.tnv; do \
	  target/debug/tnviz layout $$f --svg $(PREVIEW_DIR)/geometry-$$(basename $$f .tnv).svg > /dev/null; \
	done

.PHONY: runtime-preview

# Every example through the TikZ backend and tex/tnviz-runtime.sty.
runtime-preview:
	mkdir -p $(PREVIEW_DIR)/runtime
	cargo build -q -p tnviz-cli
	for f in examples/tnv/*.tnv; do \
	  target/debug/tnviz tikz $$f -o $(PREVIEW_DIR)/runtime/$$(basename $$f .tnv).tikz; \
	done
	cp tex/runtime-preview.tex $(PREVIEW_DIR)/runtime/
	cd $(PREVIEW_DIR)/runtime && TEXINPUTS=../../tex: pdflatex -interaction=nonstopmode -halt-on-error runtime-preview.tex > /dev/null
	pdftoppm -r 150 -png $(PREVIEW_DIR)/runtime/runtime-preview.pdf $(PREVIEW_DIR)/runtime/page

.PHONY: paper

# examples/paper.tex through LaTeX and tnviz until the label sizes settle.
PAPER := $(PREVIEW_DIR)/paper
paper:
	mkdir -p $(PAPER)
	cargo build -q -p tnviz-cli
	cp examples/paper.tex $(PAPER)/
	cp -r examples/tnv $(PAPER)/
	cd $(PAPER) && for run in 1 2 3 4; do \
	  TEXINPUTS=../../tex: pdflatex -interaction=nonstopmode -halt-on-error paper.tex > /dev/null || exit 1; \
	  grep -q "rerun tnviz\|not computed yet\|out of date" paper.log || break; \
	  ../../target/debug/tnviz tex paper || exit 1; \
	done
	pdftoppm -r 150 -png $(PAPER)/paper.pdf $(PAPER)/page
