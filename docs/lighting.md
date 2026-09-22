# The lighting model

This document specifies how the engine colours every surface of a figure
(`crates/tnviz/src/lighting`).  The language defines only the parameters
(docs/language.md, section 8.10); the formulas here belong to the engine and
are shared by every backend (docs/architecture.md, section 2).

## 1. Colours without resolving colours

The engine never turns a colour name into RGB.  Every base colour (an xcolor
expression such as `blue!60` or a document colour) is collected into a
**slot**.  Everything derived from it is one of:

- a **mix**: a slot with white or black, written as xcolor's
  `<slot>!<keep·100>!white` or `…!black`, that is
  `keep·c + (1 − keep)·other` per channel;
- a **shading program**: an expression over the point (x, y), in page
  coordinates and layout units, whose parameters are the channels of slots.
  It compiles to a PostScript calculator function (PDF function type 4);
- a **label colour rule** (section 5).

A backend supplies each slot's RGB.  The TeX runtime gets it from xcolor
(`\tnvR`, `\tnvG`, `\tnvB`; docs/protocol.md), and a raster backend
uses its own evaluator.  Evaluating a program in Rust and running its
compiled code give the same result up to the 6-decimal printing of constants.

A shading covers a square with half-size `extent` around `center`, taken
from the bounding box of the surface plus a margin of .05.

## 2. The world light

`light` gives the light's direction in the page plane, in degrees
(default 135°, from the upper left).  The light is fixed in the world. When
a tensor rotates, its outline rotates but the light does not, so a different
edge catches the light.  Shadows fall the opposite way.

Lengths below are layout units, tuned at 1 unit = 1 cm.  Line widths given
in em use the document font size (`em_pt / unit_pt`).

## 3. Tensors

### 3.1 Defaults

| Item | Value |
|---|---|
| base colour | `black!48` |
| outline | base 76% with black, width .055 em |
| shadow | base 60% with black, opacity .12, offset .1 away from the light; drawn unless `shadow=off` |

### 3.2 Polygons: rectangles, triangles, diamonds

Polygons do not get the orb's radial hotspot.  Their lighting follows the
shape.  For every point the program computes a **depth** below the outline
and an outward **normal**:

1. **Edges.**  For each edge k with outward normal nₖ, take the signed
   distance sₖ to the line of the *fillet-centre polygon*: the outline's
   polygon moved inwards by the corner radius r.
2. **Blend.**  Let s = maxₖ sₖ.  The weights are
   wₖ = exp((sₖ − s) / β), with β = FAN·max(0, −s) + 10⁻⁴ and FAN = 2.  The
   normal is the weighted mean of the nₖ, and the depth is
   r − Σ wₖsₖ / Σ wₖ.  Near the outline one edge dominates.  Deeper
   inside, the weights fan out, so the normal turns gradually and there is
   no crease along the bisectors.  (A log-sum-exp blend was tried and
   rejected because it leaves pyramid-shaped artefacts.)
3. **Fillets.**  Inside the wedge of a rounded corner, between the normals
   of its two edges, the normal points radially from the fillet's centre
   and the depth is r − ρ.  The rim bends smoothly around the corner.
4. **Facing.**  f = n̂·L, the cosine between the normal and the light.
   For the blend, n̂ is the weighted mean of the nₖ normalised, and f is
   multiplied by smooth(clamp(|mean| / .4)).  Deep inside, the normals of
   many edges cancel, so the direction is undefined there and the facing
   fades to 0.  Two edges blended equally keep a mean of at least .5 (for a
   triangle), so rims and corners are not affected.

From depth and facing:

| Term | Formula |
|---|---|
| rim width | w = h·(.0562 + .18·r), where h = `highlight-size` / .22 |
| specular rim | S = .88 · smooth(clamp(1 − \|depth − inset\| / w)) · max(0, f)² |
| back rim | B = .46 · smooth(clamp(1 − depth / (1.8·w))) · max(0, −f) |
| body gradient | g = clamp(½ + ½·(p − centre)·L / reach), where L is the light's direction and reach is the shape's extent along L |

Here `smooth(t) = t²(3 − 2t)`, `clamp` limits to [0, 1], and `inset` is
`highlight-inset`.  The default `highlight-size` .66 makes h = 3.

The colour, per channel c of the base colour, is built in three layers:

```
deep  = .78c            (78% with black)
lit   = .80c + .20      (80% with white)
body  = deep + (lit − deep)·g
spec  = .06c + .94      (6% with white)
back  = .35c            (35% with black)
out   = [body·(1 − S) + spec·S]·(1 − B) + back·B
```

The result:

- the brightest band runs along the edges that face the light;
- edges facing away are mostly darkened;
- the inside keeps only a weak, large-scale gradient.

There is never an elliptical hotspot.  A longer or shorter shape changes
the reflection along with it, because every term is measured from the
outline.

### 3.3 Orbs and dots

An orb is shaded as a sphere, in the colours of the glass faces
(section 3.2).  At each point, with R the radius and (x, y) the offset from
the centre over R, the normal is n = (x, y, √(1 − x² − y²)).  The light is
raised 20° above the page, L = (cos 20°·cos φ, cos 20°·sin φ, sin 20°), and
H is the half vector between L and the viewer.  From n come the tones of a
lit sphere:

| Tone | Formula |
|---|---|
| light | w = clamp((n·L + .2) / 1.2), wrapped Lambert |
| body | core shadow `.66c` at w = 0, the base colour c at w = ½, `.70c + .30` at w = 1, linear between |
| reflected light | Rf = .6 · smooth(clamp(1 − n_z / .3)) · max(0, −(x, y)·L̂), towards c, where L̂ is the light's page direction |
| glint | G = .85 · max(0, n·H)⁴⁰, towards `.06c + .94` |

The core shadow lies inside the silhouette, and reflected light brightens
the shadow side's edge again; together with the small glint they make the
orb read as round.  No tone mixes towards black beyond `.66c`, so light
colours stay clean.  (Two earlier versions were rejected: radial stops
towards black looked grey and dirty, and a flat gradient with rims along
the outline looked like a disc.)

## 4. Bonds and legs

A line is a stroke in its base colour (default `black!72`) at its width.

A **tube** (default base `black!42`) is shaded as a half-cylinder about its
centreline:

- For each point the program finds the nearest point of the centreline,
  across its straight and arc pieces.  The offset d from it, divided by the
  tube radius, gives the normal's in-plane part (nx, ny), and
  nz = √(1 − nx² − ny²).
- The light is raised 38° above the page: L = (cos 38°·cos φ,
  cos 38°·sin φ, sin 38°), where φ is `light`.  H is the half vector
  between L and the viewer (0, 0, 1).
- Per channel: min(1, c·(.40 + .72·max(0, n·L)) + .62·max(0, n·H)²⁶).
- The outline is the base 62% with black, width .04 em.

The highlight follows the tube's axis through bends and arcs, on the side
facing the light.

## 5. Label colours

Unless `label-color` names a colour, a label's colour is chosen from its
background:

| Label | Rule |
|---|---|
| on a tensor | `black` if the tensor's luminance > .5, else `white!92!black` |
| on a tube | the same, with luminance scaled by .85 and threshold .45, because the text sits on the bright front |
| beside a line | `black` |

The luminance is .2126 R + .7152 G + .0722 B of the base colour.  The engine
sends the rule itself (background slot, weights, threshold, the two colours),
and the backend applies it once the RGB is known (`\tnvAutoColor`).

## 6. Origin

The formulas come from the frozen TeX prototype (`tex/prototype/tnviz.sty`).  They are
now computed by the engine, so the prototype's TeX-side PostScript builder
(audit T4) and colour derivations (T7) are no longer needed.
