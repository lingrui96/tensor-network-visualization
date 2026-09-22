# Architecture

Status: proposal, 2026-09-22.  Section 6 audits the repository against it.

The system has four parts.  Each owns one kind of decision; the others may
use that decision but never make it.

```
                    ┌────────────────────────────────────────┐
                    │ 1. tnv language (docs/language.md)      │
                    │    what a figure means                  │
                    └───────────────┬────────────────────────┘
                                    │ implements
 ┌──────────────────┐   ┌──────────▼─────────────────────────┐   ┌──────────────────────┐
 │ 3. Rust API      │──▶│ 2. engine (Rust)                    │──▶│ 4. TeX layer         │
 │ programs build   │   │ front end → model → layout →        │   │ LaTeX interface and  │
 │ networks         │   │ geometry → order → backends         │◀──│ runtime              │
 └──────────────────┘   └─────────────────────────────────────┘   └──────────────────────┘
                                   .tnv in, .tikz out; label sizes (.tnvm) back
```

## 1. The tnv language

The language is the only source of meaning.  A figure means what the
specification says, whichever program reads it.

It owns, as declarations with defined semantics:

- **syntax**: the simple syntax, the index syntax, and how the first expands
  into the second (automatic index names, `Link` and `Site` tags, reuse of an
  existing bond by `A - B`);
- **structure**: the index model: tensors, indices, bonds, open legs;
- **layout intent**: `at`, relative placement, `chain`, `grid`, `stack`,
  `tree`, `rotate`, `via`, leg directions, and scene-level spacing;
- **geometry semantics**: sizes and their defaults, what a corner radius is and
  how it is clamped, fillets of bends and their minimum radius, ports and
  stubs, the length of an open leg, parallel bonds and loops, hops, where a
  label sits along a bond;
- **appearance parameters**: colour, line or tube, light, highlight size, label
  content and placement.  The language says what a parameter controls; the
  formulas that realise it belong to the engine's lighting model (section 2);
- **the attribute registry**: every attribute's category (layout, geometry,
  appearance), type, unit, default, and the stage that reads it.  Unknown
  attributes are errors.

It does not own how anything is computed or drawn.

Colour values are xcolor expressions passed through unchanged; their meaning is
xcolor's.  A tensor's default label is its name; how a name with subscripts is
typeset is the backend's choice.

## 2. The engine

The engine is the Rust implementation of the language.  Every stage implements
part of the specification and invents no rules of its own.

| Stage | Input | Output |
|---|---|---|
| front end | tnv source | the model; syntax and semantic errors with positions |
| model | — | tensors, indices, groups; layout statements; style rules with the cascade; validated against the attribute registry |
| layout | model | positions, orientations, leg directions, route points, in layout units |
| geometry | layout, label sizes | outlines, bond–outline contacts, filleted centrelines, tube outlines, hops, label positions, the lighting geometry of each surface |
| order | geometry | fragments with sort keys (pass, depth, class, z, order), split where depth order changes |
| backends | ordered fragments | output formats |

It also prints canonical tnv, and can print a resolved form in which every
computed position and route is written out as `at` and `via`.

### Lighting model

The engine owns the lighting model (docs/lighting.md), shared by every
backend so that all outputs look alike:

- every colour derived from a base colour (the glass body, rims, outlines,
  shadows, tube shades) as a formula of the base colour's RGB;
- every shading function: the geometric part (depth and normal of each point)
  together with how it combines with those colours;
- the rule that chooses a label colour from its background.

The formulas take the base colours' RGB as parameters.  The engine never needs
their values: a backend supplies them, the TeX runtime from xcolor and the
SVG backend from its own evaluator.  The model is documented with the engine,
and the language only names the parameters it exposes.

### Backends

- **TikZ backend**: writes code for the TeX runtime (section 4.2).  Colour
  expressions and label text pass through unchanged.
- **SVG/PNG backend** (later): has no TeX, so it needs its own evaluator for a
  subset of xcolor and its own text measurement.  These live in that backend
  only.

The engine does not decide what a figure means, and it does not resolve
colours or typeset text for LaTeX output.

### Crate structure

The engine and the Rust API are one crate, `tnviz`.

- Stages are modules.  Their internal data is `pub(crate)`; only the crate
  root exposes the public API of section 3, so the boundary is enforced by the
  compiler rather than by convention.
- Backends are optional Cargo features.  A library that only builds networks
  and writes tnv does not compile rasterisers or font measurement.
- A backend moves to its own crate only if its dependencies grow heavy enough
  to burden other users.
- The command-line tool is a separate crate, `tnviz-cli`, that only connects
  stages.

## 3. The Rust API

The Rust API is the facade that other programs use, for instance a
tensor-network library that wants to draw its networks.

- It builds the model in the index model's own terms: tensors, indices, tags,
  prime levels.  Conveniences such as chains are available, but they are
  defined as the corresponding tnv statements, so the API and the language stay
  equivalent: every network built through the API prints as tnv, and reading
  that tnv back gives the same network.
- `ToNetwork` lets a library hand over its structure.
- It runs the pipeline (`layout`, `render` with a backend) and writes `.tnv`.
- It exposes the engine's results, not its internals: stage data structures
  are read-only, and model invariants can only change through checked
  operations.

It does not add semantics: nothing is possible through the API that tnv cannot
express.

## 4. The TeX layer

### 4.1 LaTeX interface

- `\begin{tnvfigure} … \end{tnvfigure}` and `\tnvinput{file.tnv}` write the
  figure's tnv to a file and read back the engine's `.tikz`.
- LaTeX typesets every label and writes its size to `\jobname.tnvm`, which the
  geometry stage reads on the next run.
- The workflow is BibTeX-like: LaTeX, `tnviz`, LaTeX.

### 4.2 Runtime

A small package that draws what the engine computed:

- paths, fills, strokes, and clips at given coordinates;
- colours: the runtime resolves each base colour to RGB with xcolor,
  including colours defined in the document, and substitutes it into the
  expressions the engine emits: the shading functions (evaluated by the PDF
  viewer), derived fill and stroke colours, and the label-colour test.  It
  defines no colour and no rule of its own; it only resolves names and
  evaluates what it is given;
- text nodes, in the document's fonts.

The runtime's macros form a versioned protocol between the TikZ backend and
TeX, specified in `docs/protocol.md`.

It does not lay out, compute geometry, order fragments, or interpret tnv.  It
changes nothing outside tnv figures.

### 4.3 The prototype package

`tex/prototype/tnviz.sty` is the prototype that preceded this architecture;
it moved there when `tex/tnviz.sty` became the LaTeX interface.  Until the
engine's geometry and TikZ backend can replace it:

- it is frozen: no new features and no changes to its rules;
- its lighting formulas are the first version of the engine's lighting model;
  once ported, the engine's documentation is authoritative;
- its example images are the visual reference for comparing engine output.
  Where the two differ, the difference is judged as a fix or a regression; the
  prototype is not assumed to be right.

Afterwards it shrinks to the runtime of section 4.2 and its user interface is
retired.

## 5. Contracts between the parts

| Between | Contract |
|---|---|
| language → engine | `docs/language.md`, including the attribute registry and geometry semantics |
| engine → Rust API | the public API of the `tnviz` crate |
| TeX → engine | `.tnv` files, and `.tnvm` with the list of figures and label sizes (`docs/protocol.md`) |
| engine → TeX | `.tikz` files in the runtime protocol (`docs/protocol.md`) |

## 6. Audit of the repository, 2026-09-22

The repository was built before this architecture.  Its prototype TeX package
plays three parts at once, and the Rust crate mixes the engine with the API.

### 6.1 Language (docs/language.md)

Addressed by language v0.3 and `docs/protocol.md`: L1 (section 8), L2
(section 10), L3 (sections 8.10 and 12.3), L4 (section 10.3), L5 (section
7.2), and L6.  The implementation has not caught up yet.

| # | Finding | Should be |
|---|---|---|
| L1 | Geometry semantics are not specified.  The rules exist only in the TeX prototype: corner radii clamped to half an edge, a warning (not an error) when a bend is tighter than a tube, the hop shape, ports that run straight for a stub, label positions along the visible length only. | A geometry section in the language specification. |
| L2 | The attribute table mixes layout (`via`, `leg-dir`, `rotate`, `leg-length`), geometry (`width`, `corner-radius`, `cap`, `hop-radius`), appearance (`color`, `highlight-size`), and one TeX command (`label-font`), with no category, consumer, or validation. | An attribute registry; unknown attributes are errors. |
| L3 | Section 11.2 says the Rust side computes the shadings but not how colours reach it, while colours must be resolved by TeX to support document colours. | The lighting model in the engine with RGB parameters (section 2); TeX resolves colour names only (section 4.2). |
| L4 | It specifies that `T[1,2]` is shown as $T_{1,2}$, a typesetting decision. | The default label is the name; the backend typesets it. |
| L5 | Spacing has no place in the language, although it is a layout-unit quantity. | A scene-level layout attribute. |
| L6 | The engine–TeX protocol and the `.tnvm` format are only sketched. | Documented contracts (section 5). |

### 6.2 Engine and Rust API (crates/tnviz)

| # | Finding | Should be |
|---|---|---|
| E1 | `Network::connect` and `Network::add_open_leg` apply simple-syntax rules: automatic `_link`/`_site` names, `Link`/`Site` tags, reuse of an existing bond (model.rs, lines 338–367).  Programs using the API get syntax sugar they did not ask for. | Front end (`lang::lower`); the model offers plain operations. |
| E2 | Leg directions from `A: leg down` are stored as style rules (lower.rs, line 255) and read back by layout (layout.rs, line 738); `via` and `rotate` also travel through the style cascade. | Layout data in the model's layout part; the syntax may stay the same. |
| E3 | Attribute values are never validated; a misspelt key is silently ignored. | Validation against the registry in the model stage. |
| E4 | Layout computes shape-dependent geometry: loop size (layout.rs, line 784) and parallel-bond offset (line 789) are fixed numbers, and open-leg length is used without knowing the outline. | Layout gives route intent; geometry sizes loops, offsets, and legs from the shapes. |
| E5 | Spacing can be set only through `LayoutOptions` in Rust. | A language attribute (L5); options only as a fallback. |
| E6 | Front end, model, and layout share one crate, and all model fields and modules are public, so API users can reach engine internals and break invariants (`rules`, `layout`, `scene` are public fields). | Engine stages behind module or crate boundaries; a curated public API (section 3). |
| E7 | `model::Layout` (statements) and `layout::Layout` (results) share a name. | Distinct names. |
| E8 | Missing: geometry, order, backends, `.tnvm` input, resolved printing. | To be built (section 2). |

Placed correctly: the parser, lowering, and canonical printer; the style
cascade; the layout algorithm itself (rigid blocks, multidimensional scaling,
stress majorization, orientation).

Resolved: E1 (the simple syntax's rules are in `lang::lower`), E2 and E3
(the registry validates every rule, and stages read attributes through its
typed accessors; language v0.3 settles that all categories share one
cascade), E4 (layout records parallel bonds and loops as route intents and
leaves sizes and leg lengths to geometry), E5 (the `spacing` statement), E6
(stages are private modules; the crate root exports the API, and the model
changes only through checked operations), E7 (`LayoutStmt` and
`Placement`).  E8 is in progress: the geometry and order stages are built
for 2D, the lighting model is built and documented in docs/lighting.md,
the TikZ backend writes the runtime protocol, and `tnviz tex` reads
`.tnvm`; resolved printing remains.

### 6.3 CLI (crates/tnviz-cli)

| # | Finding | Should be |
|---|---|---|
| C1 | `debug_svg` draws pictures inside the command-line tool, with its own port length (main.rs, lines 80–137). | A backend; the CLI only connects stages. |

Resolved: `debug_svg` is a backend module behind the `debug-svg` feature.

### 6.4 TeX layer (the prototype, now tex/prototype/tnviz.sty)

The prototype is a complete renderer with its own user interface, so it plays
the language, the engine, and the runtime.

| # | Finding | Should be |
|---|---|---|
| T1 | Its own vocabulary, different from tnv: `corner radius` against `corner-radius`, absolute lengths against layout units, page-angle ports (`from=down`) against local leg directions, a `light angle` on every object against a scene light.  Defaults are duplicated. | tnv only; the runtime has no user vocabulary. |
| T2 | Engine work in TeX: outline contacts (`\tnv@boundary`, line 579), bond fillets (`\tnv@buildcenterline`, line 725), crossings and hops (`\tnv@findcrossings`, line 1424; `\tnv@rebuildbond`, line 1555), label placement (`\tnv@pointatlength`, line 1279), fragment ordering (line 515). | Engine geometry and order stages. |
| T3 | Its geometry rules are the only definition of them (see L1). | The language specification. |
| T4 | Its lighting functions compute depth and normals from the outline in PostScript code built by TeX. | Complete shading functions from the engine's lighting model. |
| T7 | It derives the glass, rim, outline, and shadow colours from the base colour with xcolor mixes (`\tnv@preparestyle`), and chooses label colours by a luminance rule of its own. | Derivations and the label-colour rule in the engine's lighting model; TeX supplies only the base colour's RGB. |
| T5 | It changes every `tikzpicture` in the document: global pgf layers (line 456) and an end-of-picture hook (line 543). | Effects confined to tnv figures. |
| T6 | Missing: `tnvfigure`, `\tnvinput`, label measurement, the runtime protocol. | LaTeX interface and runtime (section 4). |

Placed correctly: resolving colour expressions with xcolor, including
document colours, and typesetting text in the document's fonts.

Resolved by the new TeX layer, which replaces the prototype rather than
changing it: T1–T4 and T7 (`tex/tnviz-runtime.sty` has no vocabulary,
geometry, ordering, or lighting of its own; it evaluates the engine's
output), T5 (the runtime and `tex/tnviz.sty` act only inside tnv figures),
and T6 (`tnvfigure`, `\tnvinput`, label measurement through `.tnvm`, and
the runtime protocol).

### 6.5 Repository

| # | Finding | Should be |
|---|---|---|
| P1 | `.preview/` holds images that `make preview` does not produce (`highlight-*`, `large-radius`, `layout-*`). | Generated by the build or removed. |
| P2 | All rendered examples exercise the prototype's own interface; no tnv file is rendered. | Examples written in tnv, rendered through the engine. |
