# The tnv language — draft specification (v0.2)

Status: draft for discussion. Section 12 records decided and deferred
questions.

Changes in v0.2:
- The core data model is now an index model, which interoperates directly
  with ITensors.jl and tensor4all-rs (see section 2).
- The simple graph syntax is the main form; the index syntax is the core and
  the exchange format.
- Lengths are relative; the outer layer decides the physical size.

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
- The compiler computes layout, geometry, and drawing order; backends only
  produce output.
- All text (tensor names, labels) is typeset by LaTeX, in the document's
  fonts.

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
8. **Not a general programming language:** only arrays, ranges, and
   wildcards; complex generation belongs in the Rust API.

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

## 6. Simple syntax (main form)

This is the everyday hand-written form.  The compiler expands it into the
index model of section 8: every bond creates an index tagged `Link`, and
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

### 6.5 Examples

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

**3D**
```
3d, camera 35 25
grid T 2x2
T[*]: leg +z
T[*] [shape=box]
```

## 7. Layout and directions

- **Leg directions** are given in the tensor's local frame and turn with the
  tensor:
  - 2D: `up`, `down`, `left`, `right`, `up-left`, and so on, or an angle;
  - 3D: `+x`, `-y`, `+z`, or a vector such as `(0, 0, 1)`.
- **Without a direction:** a bond points towards the tensor it connects to;
  an open leg follows its prime level, prime level 0 pointing down and 1
  pointing up.  An MPO's `s` and `s'` therefore separate on their own, and so
  do bra and ket.
- **Bond routes:** `A - B [via=(2, 1), (3, 1)]`.  Corners are filleted
  automatically, and crossings may `hop`.

## 8. Index syntax (core and exchange format)

Rust exports this form.  It can also be written by hand when indices must be
matched exactly.

```
tnv 0.2

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
- Quantum-number direction: `index l[1] [arrow=out]` (the arrow points from
  the Out side to the In side).
- The simple syntax and the index syntax may be mixed in one file; both
  produce the same model.

## 9. Style and inheritance

### 9.1 Selectors and precedence

| Selector | Examples |
|---|---|
| type | `*` (every tensor), `-` (every bond), `leg` |
| tag | `tag:Site`, `tag:Link` |
| group | `ket`, `op` |
| name | `A[3]`, `A[*]`, `T[*, 1]` |
| one bond | `A[3] - A[4]`, or its index name `l[3]` |
| built-in class | `leg.open`, `bond.hop` |

Precedence from low to high: type < tag / group < name.  At the same level,
later rules override earlier ones.

### 9.2 Bond label content

Besides literal text (`label=$\chi$`), a label can be generated from the
index:

```
- [label=dim]                  // the dimension, such as 8
- [label=name]                 // the index name, such as l_{3}
- [label=dim+prime]            // dimension and prime level
```

No label is shown by default.

### 9.3 Attributes (matching the current TeX implementation)

Lengths are in layout units unless stated otherwise.

**Tensors**

| Attribute | Values | Default |
|---|---|---|
| `shape` | 2D: `orb`, `rect`, `triangle`, `diamond`, `dot`; 3D: `sphere`, `box`, `dot`, … | `rect` |
| `width`, `height` | layout units | fitted to the name |
| `corner-radius` | layout units | `.2` |
| `rotate` | angle | `0` |
| `color` | colour | `black!48` |
| `label` | text; `none` hides it | the name, so `T[1,2]` shows $T_{1,2}$ |
| `label-color`, `label-font` | — | chosen from the fill |
| `highlight-size`, `highlight-inset` | number, layout units | `.66`, `0` |
| `z` | number | `0` |

**Bonds and legs**

| Attribute | Values | Default |
|---|---|---|
| `style` | `line`, `tube` (`[tube]` is short for `style=tube`) | `line` |
| `width` | line width (em) or tube diameter (layout units) | line `.09em`, tube `.22` |
| `color` | colour | line `black!72`, tube `black!42` |
| `via`, `bend-radius` | list of points, layout units | —, `.2` |
| `cap` | `round`, `flat` | `round` |
| `crossing`, `hop-radius` | `none`, `hop` (`[hop]` is short); layout units | `none` |
| `arrow` | `none`, `forward`, `backward` | from the index direction |
| `layer` | `back`, `front` | `back` |
| `z` | number | `0` |
| `label` | text, `dim`, `name`, … | none |
| `label-pos` | 0 to 1 | `.5` |
| `label-placement` | `on`, `beside` | tube `on`, line `beside` |
| `label-side` | `auto`, `left`, `right` | `auto` |
| `label-along`, `label-offset` | shift along / across the bond | `0` |
| `label-shift` | shift in page coordinates | `(0, 0)` |
| `leg-dir`, `leg-length` | direction, open-leg length | automatic, `.6` |

**Scene**

| Statement | Examples |
|---|---|
| dimension | `2d` (default), `3d` |
| light | `light 135` (2D angle), `light (-1, 1, 2)` (3D vector) |
| camera (3D) | `camera 35 25` (azimuth, elevation), `camera [projection=perspective]` |

## 10. 3D

- A `3d` scene accepts only 3D shapes and a 2D scene only 2D shapes; a
  mismatch is an error.  In 3D, `orb` means `sphere`.
- Occlusion is resolved by the compiler, which orders fragments by depth; the
  language does not describe what is in front.  `z` only breaks ties at
  equal depth.

## 11. Entry and exit points

### 11.1 Rust

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

### 11.2 LaTeX

```latex
\usepackage{tnviz}

\begin{tnvfigure}[name=mps, unit=1cm]
  chain A[1..6]
  A[*]: leg down
  - [tube, label=$\chi$]
\end{tnvfigure}

\tnvinput[unit=8mm]{figures/peps.tnv}
```

The workflow is the same as for BibTeX or biber:

1. **First LaTeX run:** write each figure to `\jobname-mps.tnv`, and the
   typeset size of every label to `\jobname.tnvm`.
2. **Run `tnviz`:** read `.tnv` and `.tnvm` and write `\jobname-mps.tikz`.
   This can be a latexmk rule, or LaTeX can call it directly when
   shell-escape is enabled.
3. **Second LaTeX run:** input the `.tikz` file; LaTeX typesets its text
   nodes in the document's fonts.

The `.tikz` file needs only a small runtime package (`tnviz-runtime`) that
draws the paths, shadings, and text computed in Rust; no geometry is computed
in TeX.

### 11.3 Backends

| Backend | Use | Lighting |
|---|---|---|
| TikZ/PGF | LaTeX documents | PDF functional shadings, exact (reference quality) |
| PDF | standalone figures | same |
| SVG / PNG | previews, web, notebooks | pre-rendered high-resolution images or approximate gradients; small differences allowed |

## 12. Decided and deferred

**Decided**
- **Position information in index tags is not read** (for example ITensors'
  `n=3` and `l=3`).  Tags are only style selectors; order always comes from
  the layout syntax.

**Deferred**
- **Whether `grid` connects neighbours, and the default direction of open
  legs:** any network can be drawn with explicit connections and leg
  directions, so defaults will be tuned after real use.  Sections 6 and 7
  describe the current behaviour.
- **Julia export:** not needed for now; it can be written when there is a
  use for it.
