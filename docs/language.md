# The tnv language — draft specification (v0.3)

Status: draft for discussion.  Section 13 records decided and deferred
questions.  `docs/architecture.md` describes the programs that implement the
language, and `docs/protocol.md` the files they exchange.

Changes in v0.3:
- Geometry semantics are specified (section 8).
- An attribute registry replaces the attribute tables; every attribute has a
  category, type, default, and reader, and unknown attributes are errors
  (section 10).
- Scene-level spacing (section 7).
- Tensor labels default to the name; how it is typeset is the backend's
  choice.

Changes in v0.2:
- The core data model is now an index model, which interoperates directly
  with ITensors.jl and tensor4all-rs (see section 2).
- The simple graph syntax is the main form; the index syntax is the core and
  the exchange format.
- Lengths are relative; the outer layer decides the physical size.

Parts of this version are ahead of the implementation: everything after
layout (sections 8 and 12) is specified but not yet implemented.  The
implementation reads `tnv 0.2` and `0.3` and writes `0.3`.

## 1. Role

tnv is the intermediate representation of the whole system and its single
source of truth:

```
 Rust library ───(API export)──────┐
                                   ├──▶  .tnv  ──▶  tnviz compiler (Rust)  ──▶  backends
 LaTeX author ──(inline / file)────┘      ▲        layout → geometry → order    ├─ TikZ/PGF (for LaTeX, reference quality)
                                          │                                    ├─ PDF / SVG / PNG (preview)
                     style overrides ─────┘                                    └─ .tnv (canonical form)
```

- People write it by hand and programs export it; the two are equivalent.
- The language defines what a figure means, geometry included.  The compiler
  computes it; backends only produce output.
- All text (tensor names, labels) is typeset by LaTeX, in the document's
  fonts, and all colours are resolved by xcolor.

## 2. Prior art

The index model and several naming conventions are borrowed from
[ITensors.jl](https://github.com/ITensor/ITensors.jl) (with ITensorMPS.jl and
ITensorNetworks.jl) and from
[tensor4all-rs](https://github.com/tensor4all/tensor4all-rs), whose `Index`
and `Tensor` design is itself inspired by ITensors.jl.  We read their source
code to make importing from them direct.

tnv is an independent design.  Borrowing from these projects does not mean
following them: tnv does not track their changes, and its syntax, defaults,
and appearance are decided by tnv alone, with paper-quality figures as the
goal.

## 3. Design principles

1. **Declarative:** describe what the network is and how it looks, not how to
   draw it.
2. **Simple first:** the simplest figure states only connectivity; leg names,
   positions, and styles are added only when needed.
3. **Three separate layers:**
   - structure: tensors and indices, with no geometry;
   - layout: positions, orientation, leg directions, bond routes;
   - style: shapes, colours, lines or tubes, labels.

   When a program exports the structure again, layout and style attach by
   name and are not lost.
4. **Stable names:** every later adjustment refers to objects by name.
5. **3D from the start:** coordinates may have a z component; 2D is the case
   z = 0, declared by the scene.
6. **Relative units:** the language knows neither centimetres nor pixels; the
   outer layer decides the size of one unit.
7. **Round trip:** the compiler can print any .tnv file in a canonical form.
8. **A small language:** arrays, ranges, wildcards, `for` clauses, global
   variables, and functions that expand into statements (section 6.6); no
   conditionals and no values other than text.  Generation that needs more
   belongs in the Rust API.

## 4. Core data model: indices

A tensor is a list of indices, and a shared index is a contraction.  The
model is simple, and it matches the data structures of existing libraries,
so importing needs no conversion.  The table below comes from the source of
ITensors.jl and tensor4all-rs and only explains how import works; tnv is not
bound by their conventions.

### 4.1 Indices

Both libraries build an index from the same fields, and both consider two
indices equal only when id, tags, and prime level all agree.

| Field | ITensors.jl | tensor4all-rs | tnv |
|---|---|---|---|
| identity | `id::UInt64` | `DynId` | index name |
| dimension | `space` | `dim` | `dim` attribute (optional) |
| tags | `tags::TagSet` | `tags: TagSet` | classes, for selectors and styles |
| prime level | `plev::Int` | `plev: i64` | a trailing `'`, as in `s'` |
| direction | `dir::Arrow` (In / Out / Neither, for conserved quantum numbers) | none | an arrow on the bond (optional) |

Tags commonly seen on import (for reference only; tnv does not depend on
their content):

- physical indices: `Site`; ITensors adds the site position `n=3`, as in
  `"Site,S=1/2,n=3"`;
- virtual indices: `Link`; ITensors adds `l=3`, while tensor4all-rs's
  `Index::new_link` adds only `Link`;
- in ITensorNetworks the vertex `(1,2)` becomes the tag `1×2`, and an edge is
  tagged with its two vertex tags, as in `"1×2,2×2"`.

### 4.2 Tensors and contractions

- A tensor is a list of indices.
- **An index shared by two tensors is a contraction**, that is, a bond.
- An index on a single tensor is an **open leg**.
- An index belongs to one or two tensors.  A contraction of three or more
  tensors uses an explicit delta tensor, drawn as a small dot (`shape=dot`),
  so tnv needs no hyperedges.

### 4.3 Tensor names

Tensors are identified by name.  On import, the library's vertex names become
tensor names unchanged:

- tensor4all-rs's `TreeTN<T, V>` is generic in the vertex name `V`; `String`
  and `usize` are the most common;
- in ITensorNetworks.jl a vertex may be any value; grids usually use tuples
  such as `(1,2)`.

A tnv tensor name is therefore `name` or `name[subscripts]`, such as `A`,
`A[3]`, or `T[1,2]`.

- Subscripts are kept as given, with no assumption about starting at 0 or 1
  (tensor4all-rs counts from 0, ITensors.jl from 1).
- Hand-written ranges start at 1 by default, as is usual in physics papers.
- When Rust exports plain integer vertices, a configurable prefix is added,
  `T` by default, so vertex `3` becomes `T[3]`.

## 5. Units

- **Layout units:** positions, spacing, tensor sizes, and tube diameters are
  plain numbers.  The outer layer sets the size of one unit, for instance
  `unit=1cm` for the TikZ backend or `unit=40px` for SVG, and the whole
  figure scales with it.
- **Typographic units:** LaTeX typesets text at a fixed size that does not
  scale with the figure.  These sizes therefore follow the text size and are
  given in `em`:
  - line and outline widths (such as `.08em`), so lines do not thicken when a
    figure is enlarged;
  - the gap between a label and its bond;
  - the minimum size of a tensor, which always fits its own name.
- **Absolute units:** `pt`, `mm`, and so on remain available as exceptions.

Lengths default to layout units, with two exceptions that default to `em`:
line widths and gaps between labels and bonds.  The registry (section 10.3)
gives the unit of every attribute.

## 6. Simple syntax (main form)

This is the everyday hand-written form.  The compiler expands it into the
index model of section 9: every bond creates an index tagged `Link`, and
every open leg creates an index tagged `Site`.

### 6.1 Names and ranges

| Syntax | Meaning |
|---|---|
| `A` | tensor A |
| `A[3]`, `T[1,2]` | tensors with subscripts |
| `A[1..6]` | A[1] to A[6] |
| `T[1..3, 1..3]` | the nine tensors of a 3×3 array |
| `A[*]`, `T[*, 1]` | every A[…]; every T whose second subscript is 1 |

### 6.2 Connections

| Syntax | Meaning |
|---|---|
| `A - B - C` | connect in sequence |
| `chain A[1..6]` | connect neighbours in the list and place them in a row |
| `A[1..4] - W[1..4]` | pair two lists of equal length element by element |
| `R - L1, L2` | one to many |
| `grid T 3x3` | create T[1,1] to T[3,3], connect neighbours, place them on a grid |
| `A: leg down` | give A an open leg pointing down |
| `A: legs up, down` | two open legs |

### 6.3 Layout

| Syntax | Meaning |
|---|---|
| `A at (0, 0)` | explicit position |
| `B right of A`, `B above A, 2` | relative position, with an optional distance |
| `ket: chain A[1..4]` | name a whole row |
| `stack bra, op, ket` | place rows from top to bottom |
| `tree R down` | expand a tree downwards from the root R |
| (anything else) | automatic layout |

### 6.4 Style

Any selector followed by `[...]`:

```
*            [shape=orb]              // every tensor
-            [tube]                   // every bond
A[*]         [color=blue]
A[3] - A[4]  [label=$\chi$]          // one bond
tag:Site     [leg-length=.6]          // by tag
```

Math labels may be written as `$\chi$` without quotes.

### 6.5 Variables

```
let fresh = lime!55!green!75!black!55!white
let pale  = @fresh!60!white             // a variable may use earlier ones
let fade  = .6; let chi = $\chi$

A[*]         [color=@fresh, opacity=@fade]
A[3] - A[4]  [label=@chi]
leg          [color=@pale]
```

- `let name = value` declares a global variable.  The value is the rest of
  the statement as written, up to a `;`, a `//` comment, or the end of the
  line.
- `@name` in an attribute value stands for that text, anywhere in the value,
  so it can be part of a colour expression.  The value is then read as the
  attribute's type, as if it had been written there; one variable can serve
  attributes of different types.  `@` inside a string or math is text.
- A variable is declared once, before it is used.  There are no local
  variables: every variable is global.
- The canonical form (section 3, principle 7) has no variables; they are
  expanded.

### 6.6 Functions

```
def comb(hub, arm, rows, n, z) {
  chain @hub[i], @arm[i, 1..@n]   for i in 1..@rows
  @hub[i] below @hub[i-1]          for i in 2..@rows
  @hub[i-1] - @hub[i]              for i in 2..@rows
  @hub[1] at (0, 0, @z)
}

low:  comb(H, A, 4, 4, 0)
high: comb(G, B, 4, 4, 4)
low.-  [color=@sky]            // the bonds within `low`
high.- [color=@lilac]
```

- `def name(p, q, …) { … }` defines a function; its body is tnv statements.
  It must be defined before it is called, and once.
- A call `name(a, b, …)` is a statement.  Each `@p` in the body stands for
  the text of its argument, anywhere in the body (names, ranges, numbers,
  attribute values), as `@name` stands for a variable (section 6.5); an
  argument may itself use variables.  The body is then read as statements
  in place of the call.
- `g: name(…)` makes the tensors the call creates a group `g`.
- A body's own `let`s and `def`s are local to one call; its other names are
  global, like everything the call creates.  Calls may nest, up to 32 deep.
- Errors in a body give its line in the definition and the call.
- The canonical form has no functions; calls are expanded.

**`for`** repeats a statement for each integer in a range: connections,
`chain`, `at`, relative placement, `tensor`, and `index`.  Subscripts may
add to or subtract from the variable (`A[i-1]`); a coordinate of `at` may be
any arithmetic of numbers and the variable with `+`, `-`, `*`, `/`, and
parentheses, as in `A[i] at (2*i, -(i-1)/2)  for i in 1..4`.

### 6.7 Examples

**MPS**
```
chain A[1..6]
A[*]: leg down
```

**PEPS (3×3)**
```
grid T 3x3
T[*]: leg down-left
```

**⟨ψ|O|ψ⟩**
```
ket: chain A[1..4]
op:  chain W[1..4]
bra: chain B[1..4]
A[1..4] - W[1..4] - B[1..4]
stack bra, op, ket
op [shape=rect, color=teal]
```

**Tree tensor network**
```
R - L[1], L[2]
L[1] - X[1], X[2]
L[2] - X[3], X[4]
X[*]: leg down
tree R down
```

**General graph with explicit positions and a hop**
```
A at (0, 0);  B at (3, 0);  C at (1.5, 2)
A - B - C - A
D at (1.5, -1.5)
C - D [hop]
```

**3D** (section 11)
```
3d, camera -30 35
grid T 2x2
T[*]: leg -z
T[*] [shape=box]
```

## 7. Layout

Layout decides where tensors are, how they are turned, and which way legs and
bonds leave them.  All of it is in layout units.

### 7.1 Placement

- `A at (x, y)` pins a tensor.  `B right of A, d` (also `left of`, `above`,
  `below`) places one tensor relative to another, `d` layout units away.
- `chain` places its tensors in a row, `grid` on a grid with row 1 at the top,
  `stack` places the first tensors of its groups in a column from top to
  bottom, and `tree R dir` places the tensors reachable from `R` as a tree
  growing in direction `dir`, with each parent centred over its children.
- These statements fix relative positions and join tensors into rigid blocks.
  Statements that contradict one another are errors.
- Tensors that no statement places are laid out automatically.  The
  automatic layout aims at bonded tensors being `spacing` apart and is
  deterministic.  Unconnected parts are placed left to right.

### 7.2 Spacing

```
spacing 2
```

`spacing` sets the distance, in layout units, used by `chain`, `grid`,
`stack`, `tree`, relative placement without a distance, and the automatic
layout.  The default is 2.

### 7.3 Orientation and directions

- `rotate` turns a tensor about its centre, in degrees, counter-clockwise.
- **Leg directions** are given in the tensor's local frame and turn with the
  tensor:
  - 2D: `up`, `down`, `left`, `right`, `up-left`, and so on, or an angle;
  - 3D: `+x`, `-y`, `+z`, or a vector such as `(0, 0, 1)`.
- **Without a direction** (in 3D, see section 11.1): a bond heads for the next
  point of its route; an open leg follows its prime level, prime level 0
  pointing down and 1 pointing up.  An MPO's `s` and `s'` therefore separate
  on their own, and so do bra and ket.
- **Routes:** `A - B [via=(2, 1), (3, 1)]` lists points a bond passes
  through, in page coordinates.

## 8. Geometry

This section defines the shapes that layout does not: outlines, where bonds
meet them, how bends are rounded, and where labels sit.  All lengths are in
layout units unless stated otherwise.

### 8.1 Frames

The page frame has x to the right and y up.  A tensor's local frame has its
origin at the tensor's centre and is turned by its `rotate`.  Shapes, leg
directions, and loops are defined in the local frame.  The light is fixed in
the page frame and does not turn with tensors.

### 8.2 Tensor shapes

Every shape is centred on the tensor's position and fits a `width` × `height`
box in the local frame.

| Shape | Outline |
|---|---|
| `rect` | the box |
| `orb` | a circle of diameter `width`; `height` and `corner-radius` are ignored |
| `triangle` | apex at the top centre of the box, base along its bottom edge |
| `diamond` | vertices at the midpoints of the box's edges |
| `dot` | a filled circle of diameter `width`, drawn without its label; for delta tensors |

Every vertex of a polygonal shape is rounded by a circular fillet of radius
`corner-radius`, the same at every vertex.  The radius is reduced, without a
warning, to the largest value with which no fillet uses more than half of
either edge next to it; a very large radius therefore gives the roundest
shape that the edges allow.

**Default sizes.**  Without `width` and `height`, a shape takes the default
size of the registry, enlarged if needed so that the tensor's label fits with
`label-padding` on every side.  An explicit `width` or `height` is used as
given; a label that does not fit is a warning.

The **silhouette** of a tensor is its outline.  The **boundary distance** in a
direction is the distance from the centre to the silhouette along that
direction.

### 8.3 Bond centrelines

A bond is drawn along a centreline through these points, in order:

1. the centre of its first tensor;
2. if that end has a leg direction `d`, the **exit point**: the centre plus
   `d` times (boundary distance in `d` + `stub`);
3. the points of `via`;
4. the exit point of the second end, if it has a leg direction;
5. the centre of the second tensor.

Consecutive points closer than 10⁻⁴ are merged.  An end may also be a
coordinate instead of a tensor; the centreline then starts or stops there.

**Bends.**  At every interior point where the direction turns by an angle δ
(|δ| ≥ 0.5°), the corner is replaced by a circular arc tangent to both
segments.  Its radius is `bend-radius`, reduced where needed so that the arc
uses at most half of either segment:

    ρ = min(bend-radius, ½ · min(ℓ_in, ℓ_out) / tan(|δ| / 2))

A tube whose bend radius ends up smaller than the tube's radius overlaps
itself at that bend; this is a warning.

**Visible length.**  The parts of a centreline inside the silhouettes of its
end tensors are hidden under the tensors.  Positions along a bond (for labels)
are measured along the visible part only.

### 8.4 Bonds as lines and tubes

- A **line** is the centreline stroked with `width` (in `em`) and round caps.
- A **tube** is the set of points within `width` / 2 of the centreline.  Where
  the centreline ends at a coordinate rather than a tensor, the tube ends with
  a hemispherical cap (`cap=round`) or a flat cut (`cap=flat`).

### 8.5 Open legs

An open leg is a centreline from its start point in its leg direction `d`,
through the tensor's outline, and `leg-length` beyond it.  It is drawn like a
bond of its style, with its cap at the free end.

The start point is the tensor's centre moved sideways, along the unit vector
p perpendicular to `d` in reading order (in the local frame, p points right,
or down when `d` is horizontal):

- **Spread.**  The n legs of a tensor that share a direction and have no
  `leg-offset` divide the chord of the silhouette through the centre along
  p, less the corner radius at both ends, into n equal parts, in slot order;
  each starts at the middle of its part.  A leg alone in its direction
  starts at the centre.
- **`leg-offset`** starts a leg that far along p instead, and takes it out
  of the spread.  An offset beyond the silhouette starts the leg outside the
  tensor.

### 8.6 Parallel bonds and loops

- **Parallel bonds.**  When n bonds without `via` join the same two tensors,
  bond k (k = 0 … n − 1, in index order) gets one route point: the midpoint of
  the two centres, moved sideways by (k − (n − 1)/2) × `parallel-gap`.
- **Loops.**  A bond without `via` from a tensor to itself gets two route
  points in the tensor's local frame, at (± 0.35 w, h/2 + (k + 1) ×
  `loop-size`), where w × h is the tensor's box and k counts earlier loops on
  the same tensor.

### 8.7 Crossings and hops

Two centrelines cross where they intersect outside every tensor's silhouette.
At a crossing, the bond with the higher drawing key (section 8.9) is drawn
on top.

With `crossing=hop` on the upper bond, each crossing on one of its straight
pieces is replaced by a semicircular hop.  With c the crossing, e the piece's
direction, and n its normal pointing up (or left, if the piece is vertical),
the centreline passes through

    c − h e,   c − h e + (h + r) n,   c + h e + (h + r) n,   c + h e,

where h is `hop-radius`.  The two upper points get fillets of radius h, which
join into a semicircle about c + r n; the two base points get fillets of
radius r, which is 0 for a line (a sharp corner, as in circuit diagrams) and
`bend-radius` for a tube.  A crossing on a bend, or a hop that does not fit
on its piece, is left alone with a warning.

### 8.8 Labels

- **Tensor labels** are centred on the tensor and stay upright whatever its
  rotation.
- **Bond labels** sit at the fraction `label-pos` of the bond's visible
  length, then move by `label-along` along the bond and `label-offset` across
  it, and finally by `label-shift` in page coordinates.
  - `label-placement=on`: centred on the centreline and turned along its
    tangent, by a multiple of 180° chosen to keep the text upright.  A label
    taller than the tube is a warning.
  - `label-placement=beside`: upright, its box touching the line at half the
    bond's width plus `label-distance` from the centreline, on the side given
    by `label-side`.  `auto` is the side whose normal points up, or left on a
    vertical bond; `left` and `right` are relative to the bond's direction
    from its first end to its second.
  - Without `label-placement`, a label on a tube is `on` when its height
    plus depth fits the tube's diameter, and `beside` otherwise; a label on a
    line is `beside`.  Only an explicit `on` that does not fit warns.
- **Open-leg labels** follow the bond-label rules along the leg.
- **End labels** name a line's index where it meets a tensor, as in
  `- [start-label=name, end-label=name]`.  `start-label` sits near a bond's
  first tensor (or a leg's tensor) and `end-label` near a bond's second
  one; their values are those of `label`.  Each is upright, beside the line
  as for `label-placement=beside`, on the side given by `end-label-side`
  (default: `label-side`), and along the line just clear of the
  silhouette: its box's extent along the line is `end-label-inset` from
  the end of the visible part.

### 8.9 Drawing order

Every drawn piece has a key (pass, depth, class, z, order), compared in that
order, lower first:

- pass: background, scene, overlay;
- depth: distance from the viewer, far first; constant in 2D;
- class: bonds, tensor shadows, tensors, then bonds with `layer=front`;
- z: the `z` attribute;
- order: the source order.

Tensors therefore cover the ends of their bonds, and their shadows fall on
bonds.

**Opacity.**  `opacity` fades one object as a whole: a tensor with its
outline and label, or a line with its arrow and the label printed on it.
Its parts are composed first and then faded together, so they do not show
through one another; what lies behind the object shows through it,
including the ends of its bonds, which run to the tensor's centre.  A
tensor's shadow fades by the same factor, and a label beside a line fades
with the line.  Labels printed on a tube belong to the tube; labels beside bonds are
in the overlay pass.

### 8.10 Lighting

`light` sets the direction of the world light (default 135°, from the upper
left).  Surfaces facing the light are brighter and surfaces facing away are
darker; a tensor's rim highlight follows its outline and bends around its
corners, and a tube's highlight follows its axis.  `highlight-size` scales the
width of the rims and `highlight-inset` moves the brightest part inwards.
The formulas are the engine's lighting model, documented with the engine; the
language defines only these parameters.

### 8.11 Arrows

`arrow=forward` puts an arrow on a bond pointing from the first tensor that
holds its index to the second, and on an open leg pointing away from its
tensor; `backward` points the other way.  `arrow-style` chooses its form:

| Style | Form | Default for |
|---|---|---|
| `head` | a filled isosceles triangle on the centreline, .55em long and .5em wide on a line, .9 and .75 times the diameter on a tube | lines |
| `shaft` | on a tube, a head with a shaft .22 diameters wide behind it, 2.8 diameters long in all, following the centreline's bends | — |
| `beside` | a line arrow (a .09em shaft and a head) 2.4em long, parallel to the line and .3em clear of it, on the side whose normal points up (or left), or on the side away from a label beside the line | — |
| `cone` | on a tube, the tube itself becomes the arrow: it stops at the base of a cone 1.7 diameters long and 1.8 diameters wide, whose tip is at the end of the visible part it points to | tubes |

On a line, `shaft` and `cone` mean `head`.

- **Size.**  `arrow-size` scales every length above.  A shaft shortens to .8
  of the visible part, but not below 1.6 heads.
- **Position.**  `head`, `shaft`, and `beside` are centred at the fraction
  `arrow-pos` of the visible length.  Without `arrow-pos` they are centred
  on the visible part.  A label printed on the line (section 8.8) is drawn
  over a `shaft`; a `head` instead sits just past the label in the arrow's
  direction, .2em clear of it, or just before it if there is no room past
  it.  A cone
  is always at the end it points to, and a line's label is then placed on
  the visible part less the cone.
- An arrow that does not fit on the visible part is left out, with a
  warning.
- It is drawn right after its line and before the line's label; its colours
  and a cone's shading come from the lighting model.

## 9. Index syntax (core and exchange format)

Rust exports this form.  It can also be written by hand when indices must be
matched exactly.

```
tnv 0.3

index s[1..4] : Site                   // physical indices
index l[1..3] : Link                   // virtual indices

tensor A[1] (s[1], l[1])
tensor A[n] (s[n], l[n-1], l[n])  for n in 2..3
tensor A[4] (s[4], l[3])

// MPO: prime levels 0 and 1 of the same physical index; s[n]' points up
tensor W[n] (s[n], s[n]', w[n-1], w[n])  for n in 2..3
```

- An index is created when first used (such as `w[n]` above); an `index`
  statement only adds tags or a dimension.
- A shared index is a bond; no separate `bond` statement is needed.
- An index may carry a dimension: `index l[1..3] : Link [dim=8]`.
- Quantum-number direction: `index l[1] [arrow=forward]` draws an arrow from
  the first tensor that holds the index to the second (`backward` the other
  way); on an open leg, `forward` points away from its tensor.  Importers
  map a library's In and Out to these (section 8.11).
- The simple syntax and the index syntax may be mixed in one file; both
  produce the same model.

## 10. Attributes

### 10.1 Setting attributes

Attributes of every category are set the same way: a selector followed by an
attribute list, or an attribute list written at the end of the statement that
creates the object.

| Selector | Examples |
|---|---|
| type | `*` (every tensor), `-` (every bond), `leg`, `plane` (3D) |
| tag | `tag:Site`, `tag:Link` |
| group | `ket`, `op` |
| name | `A[3]`, `A[*]`, `T[*, 1]` |
| one bond | `A[3] - A[4]`, or its index name `l[3]` |
| one leg | `A.r`, `A[*].p`, `A.#2` (the second leg) |
| built-in class | `leg.open` |
| a group's bonds | `g.-`: bonds with both ends in group `g` |
| a group's legs | `g.leg`: the legs of the tensors of group `g` |

Precedence from low to high: type < tag / group (including `g.-` and
`g.leg`) < name.  At the same level,
later rules override earlier ones.  An attribute written on a creating
statement counts as a name-level rule at that position.

The category of an attribute (section 10.3) decides which stage reads it, not
how it is set: `T[*] [rotate=90]` is set like `T[*] [color=red]`, but it is
read by layout.

`[tube]`, `[line]`, and `[hop]` are short for `[style=tube]`,
`[style=line]`, and `[crossing=hop]`.

### 10.2 Values

| Type | Written as |
|---|---|
| number | `2`, `-1.5`, `.5` |
| length | a number with a unit (`2mm`, `.08em`); without a unit, the attribute's default unit |
| angle | a number, in degrees |
| direction | `up`, `down-left`, `+z`, an angle, or a vector `(0, 0, 1)` |
| points | `(2, 1), (3, 1)` |
| colour | an xcolor expression such as `blue!64!black`, passed to xcolor unchanged |
| text | `$…$` math or `"…"`; `#"…"#` when the text contains quotes |
| word | one of the listed choices |

Label text may also be generated from a bond's index: `label=dim` (its
dimension), `label=name` (its name), `label=dim+prime` (dimension and prime
level).

### 10.3 Registry

"Read by" names the stage of `docs/architecture.md` that uses the attribute.
Unknown attributes, and values of the wrong type, are errors.

**Scene statements**

| Statement | Category | Values | Default | Read by |
|---|---|---|---|---|
| `2d`, `3d` | layout | — | `2d` | layout, geometry |
| `spacing` | layout | number | `2` | layout |
| `light` | appearance | 2D: an angle; 3D: a vector in view coordinates, or `world` and a vector | 2D `135`, 3D `(-1, 1, 1)` | lighting model |
| `view` | layout | 3D: `x=` and `y=`, the world axes in view coordinates | `camera -30 35` | geometry (3D) |
| `camera` | layout | 3D: azimuth and elevation, a shorthand for `view` | `-30 35` | geometry (3D) |

**Tensors**

| Attribute | Category | Type | Default | Read by |
|---|---|---|---|---|
| `shape` | geometry | 2D: `rect`, `orb`, `triangle`, `diamond`, `dot`; 3D: `sphere`, `box`, `prism`, `octahedron`, `dot` | 2D `rect`, 3D `box` | geometry |
| `width`, `height` | geometry | length | `rect` 1.1 × .75, `orb` .8, `triangle` 1 × .85, `diamond` 1.05 × .85, `dot` .15; enlarged to fit the label | geometry |
| `thickness` | geometry | length; 3D only | see section 11.3 | geometry |
| `corner-radius` | geometry | length | `.12` | geometry |
| `rotate` | layout | angle; 3D: an angle or `(rx, ry, rz)` | `0` | layout |
| `color` | appearance | colour | `black!48` | lighting model |
| `label` | appearance | text, or `none` | the name | backend |
| `label-color` | appearance | colour, or `auto` | `auto` | lighting model |
| `label-font` | appearance | backend font; for TikZ, a LaTeX font switch such as `\small` | the backend's | backend |
| `label-padding` | geometry | length (`em`) | `.3em` | geometry |
| `highlight-size` | appearance | number | `.66` | lighting model |
| `highlight-inset` | appearance | length | `0` | lighting model |
| `shadow` | appearance | `on`, `off` | `on` | lighting model |
| `opacity` | appearance | number in [0, 1] | `1` | lighting model, backend |
| `z` | order | number | `0` | order |

**Bonds and open legs**

| Attribute | Applies to | Category | Type | Default | Read by |
|---|---|---|---|---|---|
| `style` | bonds, legs | geometry | `line`, `tube` | `line` | geometry |
| `width` | bonds, legs | geometry | length: `em` for lines, layout units for tubes | line `.09em`, tube `.22` | geometry |
| `color` | bonds, legs | appearance | colour | line `black!72`, tube `black!42` | lighting model |
| `via` | bonds | layout | points | — | layout |
| `bend-radius` | bonds, legs | geometry | length | `max(.2, .8 × width)` | geometry |
| `stub` | bonds | geometry | length | `bend-radius + width / 2` | geometry |
| `cap` | legs, bonds ending at coordinates | geometry | `round`, `flat` | `round` | geometry |
| `parallel-gap` | bonds | geometry | length | `max(.3, 1.5 × width)` | geometry |
| `loop-size` | bonds | geometry | length | `.45` | geometry |
| `crossing` | bonds | geometry | `none`, `hop` | `none` | geometry |
| `hop-radius` | bonds | geometry | length | `max(.15, .7 × (upper width + lower width) + .03)` | geometry |
| `arrow` | bonds, legs | geometry | `none`, `forward`, `backward` | `none` | geometry |
| `arrow-pos` | bonds, legs | geometry | number in [0, 1] | `.5`, or past a label on the line | geometry |
| `arrow-style` | bonds, legs | geometry | `head`, `shaft`, `beside`, `cone` | line `head`, tube `cone` | geometry |
| `arrow-size` | bonds, legs | geometry | number | `1` | geometry |
| `layer` | bonds | order | `back`, `front` | `back` | order |
| `opacity` | bonds, legs | appearance | number in [0, 1] | `1` | lighting model, backend |
| `z` | bonds | order | number | `0` | order |
| `leg-dir` | legs, bond ends | layout | direction | section 7.3 | layout |
| `leg-length` | legs | geometry | length | `.6` | geometry |
| `leg-offset` | legs | geometry | length | spread (section 8.5) | geometry |
| `label` | bonds, legs | appearance | text, `dim`, `name`, `dim+prime` | none | backend |
| `label-pos` | bonds, legs | geometry | number in [0, 1] | `.5` | geometry |
| `label-placement` | bonds, legs | geometry | `on`, `beside` | tube: `on` if the label fits, else `beside`; line: `beside` | geometry |
| `label-side` | bonds, legs | geometry | `auto`, `left`, `right` | `auto` | geometry |
| `label-along`, `label-offset` | bonds, legs | geometry | length | `0` | geometry |
| `label-shift` | bonds, legs | geometry | a point | `(0, 0)` | geometry |
| `label-distance` | bonds, legs | geometry | length (`em`) | `.15em` | geometry |
| `label-font` | bonds, legs | appearance | backend font | the backend's | backend |
| `start-label` | bonds, legs | appearance | text, `dim`, `name`, `dim+prime` | none | backend |
| `end-label` | bonds | appearance | text, `dim`, `name`, `dim+prime` | none | backend |
| `end-label-inset` | bonds, legs | geometry | length (`em`) | `.3em` | geometry |
| `end-label-side` | bonds, legs | geometry | `auto`, `left`, `right` | `label-side` | geometry |
| `label-color` | bonds, legs | appearance | colour, or `auto` | `auto` | lighting model |

Where a default refers to `width`, it means the tube diameter, or 0 for a line.

**Indices**

| Attribute | Category | Type | Default | Read by |
|---|---|---|---|---|
| `dim` | structure | positive integer | — | front end |

## 11. 3D

Status: implemented, 2026-09-22, with orthographic views.  Everything in
this section applies to `3d` scenes only; a 2D scene is unaffected by it.

### 11.1 Frame and layout

- The world frame is right-handed.  Layout works as in 2D (section 7), in the
  xy plane: `chain` along x, `grid` in xy with row 1 towards +y, `tree` and
  relative placement as in 2D, and `at` with an optional z.
- Two rules differ, because 3D figures are mostly about layers:
  - `stack` places its groups along −z (the first on top), not along −y;
  - an open leg without a direction points −z at prime level 0 and +z at
    prime level 1, not −y and +y.
- Directions: `up`, `down`, `left`, `right` (and their combinations) mean
  ±y and ±x as in 2D; `+z`, `-z`, and vectors such as `(0, 1, 1)` are also
  accepted.
- `rotate` is an angle about z, as in 2D, or three angles `(rx, ry, rz)` in
  degrees, applied about the world x, y, and z axes in that order.

### 11.2 View

The view maps the world to the page by a rotation and an orthographic
projection.

```
3d, view x=(1, -0.35, 0.3), y=(0, 0.8, 0.6)
3d, camera -30 35
```

- `view x=…, y=…` gives the directions of the world x and y axes in view
  coordinates: x to the right of the page, y up, z towards the viewer.  The
  world z axis is x × y.  The two vectors are made orthonormal: x is
  normalised, and y keeps only its part perpendicular to x.
- `camera a e` is a shorthand: the viewer is at azimuth a about the world z
  axis and elevation e above the xy plane.  At a = 0 the page's right is +x,
  the direction towards the viewer is (0, −cos e, sin e), and the page's up
  is (0, sin e, cos e); a turns all three about z.  `camera 0 90` looks
  straight down z and shows the 2D picture.
- A point's page position is the first two of its view coordinates, and its
  **depth** the third (larger is nearer).
- Without `view` or `camera`, a 3D scene uses `camera -30 35`.
- Only rotations are allowed: an oblique projection would squash spheres and
  tubes, and their lighting.

### 11.3 Shapes

A 3D scene accepts only these shapes; a 2D shape name there is an error, and
these names are errors in a 2D scene.

| Shape | Solid, in the local frame (x right, y up, z towards +z) | Default size (w × h × t) |
|---|---|---|
| `sphere` | a ball of diameter `width` | .8 |
| `box` | the box `width` × `height` × `thickness` | 1.1 × .75 × .75 |
| `prism` | the 2D `triangle` of `width` × `height` in the xy plane, extruded along z by `thickness` | 1 × .85 × .75 |
| `octahedron` | vertices at (±w/2, 0, 0), (0, ±h/2, 0), (0, 0, ±t/2) | 1.05 × .85 × .85 |
| `dot` | a small ball of diameter `width`, drawn without its label | .15 |

- **Rounding.**  A polyhedral solid P is rounded with radius r =
  `corner-radius` as (P shrunk by r) grown by r: its faces stay where P's
  are, and every edge and vertex is rounded with radius r.  r is reduced,
  without a warning, to the largest value that leaves the shrunk solid
  non-empty.  This is the 2D rounding of section 8.2 in one more dimension.
- **Cross-sections.**  The cut of `box`, `prism`, and `octahedron` by their
  local plane z = 0 is the 2D `rect`, `triangle`, and `diamond` of the same
  width, height, and radius.
- **Labels.**  A shape is enlarged for its label as its 2D cross-section
  would be (section 8.2), so its size does not depend on the view.
- **Silhouette.**  The projected outline of a sphere is a circle; that of a
  rounded solid is the convex hull of its shrunk solid's projected vertices,
  grown by r.

### 11.4 Bonds and legs

- Centrelines are built as in section 8.3, in 3D: exit points use the 3D
  boundary distance, and every bend is a circular arc in the plane of its
  corner.  Open legs are as in section 8.5.
- A line runs from its tensors' centres, as in 2D; where it is inside a
  tensor, the tensor hides it (section 11.7), so a tube looks inserted into
  its tensor, meeting its surface along their true intersection.  The
  **visible part** of a line, along which labels and arrows are placed, is
  what lies outside its end tensors.
- A **line** is drawn as its projected centreline with its width in `em`,
  whatever its depth.  A **tube** is the solid of points within `width` / 2
  of the centreline; under the orthographic view its outline is the
  projected centreline grown by `width` / 2.
- Hops (section 8.7) are 2D only: in 3D, depth tells which line is in front,
  and `crossing=hop` is ignored with a warning.
- Arrows (section 8.11) are as in 2D; a `cone` is a cone in space.

### 11.5 Labels

Labels are upright and face the viewer.  A tensor's label is at its
projected centre and drawn with the tensor, in front of its surface.  Bond,
leg, and end labels follow sections 8.8 and 8.11 on the projected
centreline, in page space.

### 11.6 Planes

A plane is a translucent sheet, for showing the layers of a network or a
region of space.  It is not a tensor: it holds no indices.

```
bra: grid B 3x3
ket: grid K 3x3
stack bra, ket
plane Lb under bra                            // fitted under a group
plane Lk under ket [color=cyan!40, padding=.5]
plane W at (0, 0, -2) [width=8, height=6]     // placed explicitly
```

- `plane <name> under <group>` fits a plane to a group: the plane through
  the group's tensor centres, covering their silhouettes plus `padding`
  (default .4) on every side, as a rectangle aligned with the plane's own
  axes (the world x and y axes projected onto it, as far as possible).
  The centres must lie in one plane; if they are on one line, the plane
  contains the line and its normal is closest to +z; for one tensor, the
  normal is +z.
- `plane <name> at <point>` places a `width` × `height` rectangle (default
  4 × 3) centred at the point, in its local xy plane, turned by `rotate`
  (section 11.1).
- Its corners are rounded by `corner-radius` (default .12).
- A plane takes the attributes of its statement, over those of rules with
  the type selector `plane`: `color` (default `black!35`), `opacity` of the
  sheet (default .18; its edge is drawn at 2.5 times that, at most 1), `z`,
  `corner-radius`, `padding` (for `under`), and `width`, `height`, and
  `rotate` (for `at`).  Labels on planes are not specified yet.
- A plane is translucent: what is behind it shows through it, and what is
  in front of it is not covered by it (section 11.7).

### 11.7 Drawing order

In 3D the engine resolves which surface is in front at every point of the
page, whatever the shapes, from two things each drawn object gives: its
region on the page, and the depth of its front surface at each point of it.
Tensors, lines, arrows, and planes are all resolved the same way:

1. **Shadows** come first: they are on the paper, under everything.
2. **Objects** (tensors and lines, with their arrows) are drawn in order of the depth of their centres, far first.
   Where two objects overlap on the page and the one drawn first is nearer
   at some points, the one drawn later has a **hole** there: it is not
   drawn where the other is in front of it.  So bonds that enter a tensor
   are hidden inside it, and a tube that crosses another or passes behind a
   tensor is covered where it should be, with no splitting of lines.
3. **Planes** are drawn last, far first, each with a hole wherever an
   object, or an earlier plane, is nearer than it.  What lies behind a
   plane is therefore seen through it, and what lies in front, or on its
   near side of an intersection, is not covered by it: a plane through the
   centres of a layer's tensors cuts them along their intersection with it.
4. **Labels** are drawn above everything, far first.  Each is a card
   facing the viewer at the depth of what it is printed on: a tensor's
   label at the tensor's front surface where the label is, a label on a
   line at the line's front surface, and a label beside a line at the
   depth of the nearest point of the line.  A card is covered only by
   tensors nearer than it, never by its own tensor, by lines, or by
   planes: so a bond that leaves a tensor towards the viewer does not hide
   the tensor's name, but a tensor in front of another hides its name as
   it hides its body.

A hole is found on a grid over the two regions' overlap and refined where
the depth order changes, so it follows intersections of curved surfaces.
It reaches a little past the later object's region, so that its outline
is cut with it.
Opacity (section 8.9) is as in 2D, and a shadow is the soft drop shadow of
section 3.1 of docs/lighting.md, offset on the page away from the light.

### 11.8 Light

`light` is a scene statement.  In 3D it is a direction in view
coordinates, so that it keeps its place in the picture whatever the view:
the default is `light (-1, 1, 1)`, from the upper left and the front.
`light world (x, y, z)` fixes it in the world instead.  Rotating an object
never turns the light.  The shading of spheres, rounded solids, tubes,
cones, and planes is the engine's lighting model (docs/lighting.md).

## 12. Entry and exit points

### 12.1 Rust

```rust
use tnviz::{Network, Figure, Backend};

// Export from a tensor-network library: only the structure layer is needed.
let net: Network = my_treetn.to_network();

let fig = Figure::new(net)
    .style_str("chain T[*]\n- [tube]\nT[*]: leg down")?;   // layout and style in tnv

fig.render(Backend::Svg, "mps.svg")?;       // preview
fig.write_tnv("mps.tnv")?;                  // for LaTeX or manual editing
```

```rust
pub trait ToNetwork {
    fn to_network(&self) -> Network;
}
```

For tensor4all-rs this is direct: each vertex of a `TreeTN<IdxTensor, V>`
becomes a tensor, and each `Index` keeps its id, tags, and prime level.

### 12.2 LaTeX

```latex
\usepackage{tnviz}

\begin{tnvfigure}[name=mps, unit=1cm]
  chain A[1..6]
  A[*]: leg down
  - [tube, label=$\chi$]
\end{tnvfigure}

\tnvinput[unit=8mm]{figures/peps.tnv}
```

LaTeX writes each figure's tnv to a file; `tnviz` computes it and writes a
`.tikz` file; LaTeX draws that file with the document's fonts and colours.
Label sizes measured by LaTeX flow back to `tnviz` so that geometry can use
them.  `docs/protocol.md` specifies the files and the order of runs.

### 12.3 Backends

| Backend | Use | Lighting |
|---|---|---|
| TikZ/PGF | LaTeX documents | PDF functional shadings from the engine's lighting model, with colours resolved by xcolor (reference quality) |
| PDF | standalone figures | same |
| SVG / PNG | previews, web, notebooks | the same model, rendered to images or approximated by gradients; small differences allowed |

## 13. Decided and deferred

**Decided**
- **Position information in index tags is not read** (for example ITensors'
  `n=3` and `l=3`).  Tags are only style selectors; order always comes from
  the layout syntax.
- **Attributes of every category share one selector syntax and cascade**;
  the category decides only which stage reads them.
- **Corner radii are clamped silently; bends tighter than a tube warn.**
- **A tube label goes beside the tube when it does not fit on it**, unless
  `label-placement=on` asks otherwise.

**Deferred**
- **Whether `grid` connects neighbours, and the default direction of open
  legs:** any network can be drawn with explicit connections and leg
  directions, so defaults will be tuned after real use.  Sections 6 and 7
  describe the current behaviour.
- **Julia export:** not needed for now; it can be written when there is a
  use for it.
- **Perspective projection** and oblique projections in 3D.
- **Planes in 2D** (a panel behind a group), styling planes by name, and
  labels on planes.
