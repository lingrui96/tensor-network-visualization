# Files between LaTeX and the engine

Status: implemented, 2026-09-22: `tex/tnviz.sty` (the LaTeX interface),
`tex/tnviz-runtime.sty` (the runtime), and `tnviz tex` (the engine side).

LaTeX and `tnviz` exchange three kinds of files.  LaTeX never interprets tnv,
and `tnviz` never typesets text or resolves colours.

| File | Written by | Read by | Content |
|---|---|---|---|
| `<job>-<figure>.tnv` | LaTeX | `tnviz` | a figure's tnv source, copied verbatim |
| `<job>.tnvm` | LaTeX | `tnviz` | the list of figures and the measured size of every label |
| `<job>-<figure>.tikz` | `tnviz` | LaTeX | the computed figure, in the runtime protocol |

`<job>` is `\jobname`; `<figure>` is the figure's `name`, or `fig<n>` for the
n-th unnamed figure.

## 1. Runs

The workflow is the same as for BibTeX or biber:

1. **LaTeX.**  Every `tnvfigure` and `\tnvinput` writes its `.tnv` file and a
   line to `.tnvm`.  A figure without a `.tikz` file yet is drawn as an empty
   box of its estimated size, with a warning to run `tnviz`.
2. **`tnviz tex <job>`.**  Reads `.tnvm`, lays out and computes every figure,
   and writes the `.tikz` files.  Labels not measured yet get estimated sizes.
   Every label records in the `.tikz` file the size that geometry used.  A
   `.tikz` file is rewritten only when its content changes, and a figure
   that fails is reported without stopping the others.
3. **LaTeX.**  Draws the `.tikz` files.  The runtime typesets every label,
   writes its size to `.tnvm`, and warns "label sizes changed; rerun tnviz"
   if any size differs from the one recorded by more than 0.01pt.
4. Repeat 2 and 3 until there is no warning, usually once more.

A latexmk rule can run `tnviz` when `.tnvm` changes.  With shell-escape
enabled, LaTeX may run `tnviz` itself at the end of step 1.

## 2. `.tnvm`

Plain text in UTF-8, one record per line.  Fields are separated by spaces;
lengths are in TeX points (`pt`), written as decimal numbers.  Lines starting
with `%` are comments.

```
tnvm 1
figure mps unit=28.45274 em=10 source=paper-mps.tnv
label mps t:A[1] 7.52 6.83 0
label mps b:_link[3] 5.71 4.31 1.94
```

| Record | Fields |
|---|---|
| `tnvm <version>` | the format version, first line |
| `figure <figure> unit=<pt> em=<pt> source=<file>` | one figure: the size of one layout unit, the em of the font around it (lengths in em, such as label padding, use it), and its tnv file, the rest of the line |
| `label <figure> <id> <width> <height> <depth>` | the typeset box of one label |

A figure name contains only letters, digits, `-`, `_`, and `.`, because it
is part of file names.  LaTeX rewrites the whole file on every run; source
paths are relative to the directory of the `.tnvm` file.

Label ids are stable across runs:

| Label of | Id |
|---|---|
| a tensor | `t:<tensor name>` |
| a bond | `b:<index name>`, with its primes |
| an open leg | `l:<index name>`, with its primes |

Names are written as in canonical tnv, such as `t:T[1,2]` or `b:s[2]'`.

## 3. Runtime protocol (`.tikz`)

The `.tikz` file is TeX code for the runtime package, input inside the
figure's `tikzpicture`.  The picture's x and y units are one layout unit, so
all coordinates are in layout units.  Nothing in the file affects the
document outside the figure.

The first command checks the protocol version:

```latex
\tnvRuntime{1}
```

### 3.1 Colours

```latex
\tnvColor{<slot>}{<xcolor expression>}
```

declares a base colour.  The runtime resolves it with xcolor, in the
document's context, to RGB.  Every later command refers to it by its slot
number:

- in colour arguments, `\tnvSlot{<slot>}` is the colour itself, so derived
  colours are xcolor mixes such as `\tnvSlot{1}!76!black`;
- in shading code, `\tnvR{<slot>}`, `\tnvG{<slot>}`, and `\tnvB{<slot>}`
  expand to its components as decimal numbers.

The mixes and the shading code come from the engine's lighting model; the
runtime evaluates them and defines none of its own.

### 3.2 Drawing

Commands are drawn in the order they appear; the engine has already sorted
them.

| Command | Draws |
|---|---|
| `\tnvFill{<path>}{<colour>}{<opacity>}` | a filled path |
| `\tnvStroke{<path>}{<colour>}{<width>}{<cap>}` | a stroked path; `<width>` is a TeX length, `<cap>` is `round` or `butt` |
| `\tnvShade{<path>}{<cx>}{<cy>}{<extent>}{<code>}` | a PDF functional shading clipped to the path, on the square of half-size `<extent>` centred at (`<cx>`, `<cy>`); `<code>` is PostScript calculator code taking page coordinates and returning RGB, using at most 100 stack entries |
| `\tnvText{<id>}{<x>}{<y>}{<angle>}{<anchor>}{<font>}{<colour>}{<w>}{<h>}{<d>}{<text>}` | a label: `<text>` typeset with `<font>`, turned by `<angle>`, with its `<anchor>` at (`<x>`, `<y>`); `<w>`, `<h>`, `<d>` record the size geometry used, for the rerun check |

Paths use TikZ path syntax in layout units: `--`, `arc[…]`, and `cycle`.
Numbers are plain decimals with no exponent.  A label's `<anchor>` is
`center`, or an angle in degrees naming the point of the label's box in
that direction from its centre (a TikZ angle anchor).  `<angle>` is in
degrees; `<w>`, `<h>`, and `<d>` are in pt, as in `.tnvm`.  `<text>` is TeX
source: math in dollars, or text with TeX's special characters escaped.

A label's `<colour>` is a colour argument as in 3.1, or

```latex
\tnvAutoColor{<slot>}{<wr>}{<wg>}{<wb>}{<threshold>}{<dark>}{<light>}
```

which chooses `<dark>` when wr·R + wg·G + wb·B of the slot's colour exceeds
`<threshold>`, and `<light>` otherwise.  The weights, the threshold, and both
colours come from the engine's lighting model.

### 3.3 Versioning

The protocol version changes whenever a command is added, removed, or changes
meaning.  A runtime refuses a `.tikz` file of a version it does not know, with
an error naming both versions.
