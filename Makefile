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
