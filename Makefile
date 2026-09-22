.PHONY: preview

PREVIEW_DIR := .preview

preview:
	mkdir -p $(PREVIEW_DIR)
	TEXINPUTS=tex: lualatex -interaction=nonstopmode -halt-on-error -output-directory=$(PREVIEW_DIR) examples/effects.tex
	TEXINPUTS=tex: lualatex -interaction=nonstopmode -halt-on-error -output-directory=$(PREVIEW_DIR) examples/bonds.tex
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
