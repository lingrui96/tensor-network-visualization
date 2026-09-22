# tnviz

This fresh prototype starts with four independent 2D glyph primitives:
`orb`, `rounded rectangle`, `triangle`, and `diamond`.

Each polygonal primitive accepts `corner radius=0` for sharp corners and a
positive radius for rounded corners.  The public `color` key is the only
colour input: the renderer derives the glass face, lit and back rims, and
shadow from that one base colour.

The current visual contract is intentionally small and shared by all glyphs:

- light angle=135 is the default upper-left world light.  It stays fixed in
  page space: rotating a glyph rotates its geometry, not the light.  Its
  opposite sets the soft shadow direction.
- The orb uses radial sphere shading.  Polygonal glyphs never use a radial or
  elliptical hotspot; their lighting is shape-aware:
  - The flat face keeps only a weak, large-scale linear gradient along the
    light direction.
  - The contour carries a narrow specular rim whose intensity follows the
    local outward normal, max(0, n.L)^2.  It is brightest on edges facing the
    light and bends continuously around each rounded corner, because the
    normal sweeps along the fillet arc.
  - Edges facing away from the light receive a darkening rim, max(0, -n.L),
    instead of a highlight.
  - The rims are derived from the actual contour, so they stretch with the
    aspect ratio.
- Each polygonal face is a single PDF functional shading evaluated per
  point, so rims are true continuous gradients.  Sharp corners (radius 0)
  keep a sharp change of normal.  Radii too large for an edge are clamped.
- Where the rims of adjacent edges meet deep inside a corner, the edge
  normals and depths are blended smoothly, so the lighting fans around the
  corner without a crease along the angle bisector.
- highlight size scales the rim width (default .66).  A larger corner radius
  gives a slightly broader, gentler rim.  highlight inset (default 0pt) moves
  the specular peak inward from the contour.

For example:

~~~tex
\tnvShape[
  shape=triangle,
  color=violet!64!black,
  corner radius=1.8mm,
  rotation=-90,
  text color=white!92!black
]{U}{(0,0)}{$U$}
~~~

The renderer uses vector outlines and resolution-independent shadings rather
than raster blur, so its details remain clean at paper-figure scale.

## Bonds

~~~tex
\tnvBond{A}{B}                                   % line bond, centre to centre
\tnvBond[style=tube]{A}{B}                       % tube bond
\tnvBond[style=tube,from=up,to=up,
         via={(0,1.6),(5.2,1.6)}]{A}{B}          % polyline tube
\tnvLeg[style=tube]{A}{down}{7mm}                % open leg with a round cap
\tnvBond[crossing=hop]{A}{B}                     % jump crossings
\tnvBond[style=tube,width=3.2mm,label=$\chi$]{A}{B}   % label on the tube
\tnvBond[label=$d$]{A}{B}                        % label beside the line
~~~

- An end is a glyph id, a TikZ node name, or a coordinate in parentheses.
  Bonds may name glyphs placed later in the same picture.
- style=line|tube; width is the stroke width of a line or the diameter of
  a tube; color is the one colour input.
- from / to name a port where the bond leaves a glyph: left, right, up,
  down, or a page angle.  The bond starts on the glyph's silhouette in that
  direction and runs straight for stub before turning.
- via lists intermediate points.  Every corner becomes a circular fillet of
  bend radius, clamped to half of each adjacent segment.  A tube warns when
  a bend is tighter than its own radius.
- A tube is a true half-cylinder lit by the same upper-left world light as
  the glyphs.  Every point is shaded from the nearest point of the
  centreline, so the lit side follows the tube around every bend.  Open
  ends are hemispherical caps (cap=round, the default) or cut flat
  (cap=flat).
- Crossings are left alone by default: the bond with the higher scene key
  (see below) is drawn on top.  With crossing=hop, that bond jumps the
  crossing with a semicircle, as in circuit diagrams: sharp-cornered for a
  line and smoothly filleted for a tube; hop radius sets its size.
  Distinct colours are another way to tell crossing bonds apart.
- label puts text on a bond.  label pos is a fraction of the visible
  length, between the two silhouettes (default .5).  label placement=on
  prints it along a tube's axis, kept upright, in black or white depending
  on the tube's brightness (the default for tubes); label placement=beside
  keeps it upright just outside the bond (the default for lines), above or
  to the left unless label side=left|right is given relative to the bond's
  direction.  label font, label color, and label distance adjust it.  A
  label printed on a tube moves with the tube in the scene order; a label
  beside a bond is drawn above the whole scene.

## Scene order

Glyphs and bonds are not drawn where they are written.  Each registers
fragments with a sort key (pass, depth, class, z, source order), and the
sorted scene is drawn when the picture ends, on a layer below TikZ's main
layer, so ordinary TikZ annotations stay on top of it.

- class is the drawing convention: bonds, then glyph shadows, then glyphs.
  A bond with layer=front is drawn above the glyphs.
- z orders objects within their class (default 0); ties follow the source
  order.
- depth is constant in 2D and reserved for 3D, where one object will own
  several fragments at different depths.  The current rounded rectangle,
  triangle, and diamond are 2D glyphs; bonds only ask a glyph for the
  distance to its silhouette in a direction, so 3D solids can plug in their
  own answer.

## Rust

The Rust side reads and writes the tnv language described in
`docs/language.md`.  It is a Cargo workspace:

- `crates/tnviz`: the engine and its Rust API.  `tnviz::parse` reads tnv
  source into a `Network` (tensors, indices, groups, layout statements, and
  attribute rules checked against the registry of `docs/language.md`), and
  `tnviz::to_tnv` prints any network in canonical tnv.  Attributes are
  resolved with `tensor_style`, `bond_style`, and `leg_style`.
- `tnviz::layout` places a network: tensor positions, open-leg directions,
  and bond routes, in layout units.
- `tnviz::geometry` builds the geometry of section 8 of the language:
  tensor outlines sized to their labels, filleted centrelines of bonds and
  legs, tube outlines, visible lengths, crossings with hops, and label
  positions.  Label sizes come from LaTeX when known and are estimated
  otherwise.  `chain`, `grid`, `tree`, `stack`, and
  relative placement join tensors into rigid blocks and `at` pins them;
  everything else is placed automatically by stress majorization, starting
  from classical multidimensional scaling.  The result is deterministic.
- `tnviz::order` sorts the pieces of a figure into drawing order
  (section 8.9 of the language).
- `tnviz::lighting` gives every surface its colours: mixes of base-colour
  slots, shading programs that compile to PostScript calculator functions,
  and label-colour rules (docs/lighting.md).
- In a `3d` scene (section 11 of the language) the same stages build solids
  (`sphere`, `box`, `prism`, `octahedron`) seen by an orthographic `view` or
  `camera`, lines in space cut into spans ordered by depth, and translucent
  planes; `examples/tnv/*3d.tnv` show them.
- `tnviz::tikz` writes a figure in the runtime protocol of
  `docs/protocol.md`, and `tex/tnviz-runtime.sty` draws it in a
  `tikzpicture`.
- `crates/tnviz-cli`: the `tnviz` command.

```sh
cargo test
cargo run -p tnviz-cli -- check examples/tnv/sandwich.tnv
cargo run -p tnviz-cli -- fmt examples/tnv/sandwich.tnv
cargo run -p tnviz-cli -- layout examples/tnv/sandwich.tnv --svg layout.svg
cargo run -p tnviz-cli -- tikz examples/tnv/sandwich.tnv -o sandwich.tikz
```

## LaTeX

`tex/tnviz.sty` puts tnv figures in a document:

```latex
\usepackage{tnviz}
...
\begin{tnvfigure}[name=mps, unit=1cm]
chain A[1..6]
A[*]: leg down
\end{tnvfigure}

\tnvinput[unit=8mm]{figures/peps.tnv}
```

Options go on the `\begin` line.  `\tnvset{unit=8mm}` sets defaults.  The
workflow is the same as for BibTeX:

```sh
pdflatex paper      # writes paper.tnvm and the figures' .tnv files
tnviz tex paper     # writes paper-<figure>.tikz
pdflatex paper      # draws the figures and measures their labels
```

If LaTeX warns "Label sizes changed; rerun tnviz" or that a figure is out of
date (its source, unit, or font size changed since `tnviz tex` computed it),
run `tnviz tex` and LaTeX once more.  `make paper`
runs `examples/paper.tex` through the whole cycle into `.preview/paper/`.

A single `.tikz` file from `tnviz tikz` can also be drawn by hand with the
runtime, inside a picture whose unit is one layout unit:

```latex
\usepackage{tnviz-runtime}
...
\begin{tikzpicture}[x=1cm, y=1cm]\input{sandwich.tikz}\end{tikzpicture}
```

`tnviz layout --svg` draws a plain picture of the layout and geometry,
through the library's `debug-svg` feature, and `make geometry-preview` draws
every file in `examples/tnv/` into `.preview/`.  The debug SVG draws flat
shapes, without the drawing order or the lighting model.  `make
runtime-preview` draws every example through the TikZ backend and the
runtime into `.preview/runtime/`, with estimated label sizes.

The prototype package that preceded the engine is frozen in
`tex/prototype/tnviz.sty`; `make preview` renders its examples.

Build locally viewable, ignored effect images with:

```sh
make preview
```

The resulting `orb.png`, `rounded-rectangle.png`, `triangle.png`,
`diamond.png`, and `bonds-*.png` live in `.preview/`.  This is intentionally
a visual-prototype stage; no Rust model or tensor-network topology API is
introduced yet.
