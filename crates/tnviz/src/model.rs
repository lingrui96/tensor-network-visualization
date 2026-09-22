//! The tensor-network model: structure (tensors and indices), layout
//! statements, and style rules.
//!
//! A tensor is a list of slots, each holding an index.  An index held by two
//! slots is a bond; an index held by one slot is an open leg.

use std::collections::{BTreeSet, HashMap};

use crate::error::{Error, Result};
use crate::name::{Name, NamePattern};
use crate::value::{Attr, Value, merge_attrs};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TensorId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IndexId(pub usize);

/// Base names of indices created by the simple syntax.  The leading
/// underscore keeps them apart from hand-written names.
pub const AUTO_LINK: &str = "_link";
pub const AUTO_SITE: &str = "_site";

#[derive(Clone, Debug, PartialEq)]
pub struct Index {
    pub name: Name,
    pub prime: u32,
    pub tags: BTreeSet<String>,
    pub dim: Option<u64>,
    holders: Vec<(TensorId, usize)>,
}

impl Index {
    /// The slots holding this index: two for a bond, one for an open leg.
    pub fn holders(&self) -> &[(TensorId, usize)] {
        &self.holders
    }

    pub fn is_bond(&self) -> bool {
        self.holders.len() == 2
    }

    pub fn is_open(&self) -> bool {
        self.holders.len() == 1
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub index: IndexId,
    /// An optional name local to the tensor, such as `r` in `A.r`.
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tensor {
    pub name: Name,
    pub slots: Vec<Slot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Dim {
    #[default]
    Two,
    Three,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Scene {
    pub dim: Dim,
    /// An angle in 2D, or a vector in 3D.
    pub light: Option<Vec<f64>>,
    pub camera: Option<Camera>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Camera {
    /// Azimuth and elevation in degrees.
    pub angles: Option<(f64, f64)>,
    pub attrs: Vec<Attr>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    RightOf,
    LeftOf,
    Above,
    Below,
}

/// A direction: a word such as `down-left`, an angle, a 3D axis such as
/// `+z`, or a vector.
#[derive(Clone, Debug, PartialEq)]
pub enum Direction {
    Word(String),
    Angle(f64),
    Axis(String),
    Vector(Vec<f64>),
}

impl Direction {
    pub const WORDS: [&'static str; 8] =
        ["up", "down", "left", "right", "up-left", "up-right", "down-left", "down-right"];

    pub fn to_value(&self) -> Value {
        match self {
            Direction::Word(w) | Direction::Axis(w) => Value::Word(w.clone()),
            Direction::Angle(a) => Value::Number(*a),
            Direction::Vector(v) => Value::Points(vec![v.clone()]),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Layout {
    At { tensor: TensorId, pos: Vec<f64> },
    Relative { tensor: TensorId, relation: Relation, anchor: TensorId, distance: Option<f64> },
    Row { tensors: Vec<TensorId> },
    Grid { base: String, rows: usize, cols: usize },
    Stack { groups: Vec<String> },
    Tree { root: TensorId, direction: Direction },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub name: String,
    pub members: Vec<TensorId>,
}

/// How a style rule picks a leg of a tensor.
#[derive(Clone, Debug, PartialEq)]
pub enum LegKey {
    Label(String),
    /// 1-based slot position, written `A.#2`.
    Position(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Selector {
    /// `*`
    Tensors,
    /// `-`
    Bonds,
    /// `leg`: every slot.
    Legs,
    /// `leg.open`
    OpenLegs,
    /// `tag:Site`
    Tag(String),
    /// A name pattern; matches groups, tensors, and indices by name.
    Name(NamePattern),
    /// A leg of the tensors matching the pattern, such as `A[*].p`.
    LegOf(NamePattern, LegKey),
    Tensor(TensorId),
    Index(IndexId),
    Slot(TensorId, usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub selector: Selector,
    pub attrs: Vec<Attr>,
}

/// A tensor network with its layout and style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Network {
    pub scene: Scene,
    tensors: Vec<Tensor>,
    tensor_by_name: HashMap<Name, TensorId>,
    indices: Vec<Index>,
    index_by_key: HashMap<(Name, u32), IndexId>,
    groups: Vec<Group>,
    pub layout: Vec<Layout>,
    pub rules: Vec<Rule>,
    auto_links: i64,
    auto_sites: i64,
}

impl Network {
    pub fn new() -> Self {
        Self::default()
    }

    // ---- Queries ---------------------------------------------------------

    pub fn tensors(&self) -> impl ExactSizeIterator<Item = (TensorId, &Tensor)> {
        self.tensors.iter().enumerate().map(|(k, t)| (TensorId(k), t))
    }

    pub fn indices(&self) -> impl ExactSizeIterator<Item = (IndexId, &Index)> {
        self.indices.iter().enumerate().map(|(k, i)| (IndexId(k), i))
    }

    pub fn tensor(&self, id: TensorId) -> &Tensor {
        &self.tensors[id.0]
    }

    #[allow(clippy::should_implement_trait)]
    pub fn index(&self, id: IndexId) -> &Index {
        &self.indices[id.0]
    }

    pub fn find_tensor(&self, name: &Name) -> Option<TensorId> {
        self.tensor_by_name.get(name).copied()
    }

    pub fn find_index(&self, name: &Name, prime: u32) -> Option<IndexId> {
        self.index_by_key.get(&(name.clone(), prime)).copied()
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    pub fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.name == name)
    }

    /// Indices shared by two slots.
    pub fn bonds(&self) -> impl Iterator<Item = (IndexId, &Index)> {
        self.indices().filter(|(_, i)| i.is_bond())
    }

    /// Indices held by a single slot.
    pub fn open_legs(&self) -> impl Iterator<Item = (IndexId, &Index)> {
        self.indices().filter(|(_, i)| i.is_open())
    }

    /// The tensors matching a pattern, in name order.
    pub fn match_tensors(&self, pattern: &NamePattern) -> Vec<TensorId> {
        let mut ids: Vec<TensorId> =
            self.tensors().filter(|(_, t)| pattern.matches(&t.name, 0)).map(|(id, _)| id).collect();
        ids.sort_by(|a, b| self.tensor(*a).name.cmp(&self.tensor(*b).name));
        ids
    }

    // ---- Building --------------------------------------------------------

    /// The tensor with this name, created without slots if missing.
    pub fn tensor_or_create(&mut self, name: &Name) -> TensorId {
        if let Some(id) = self.find_tensor(name) {
            return id;
        }
        let id = TensorId(self.tensors.len());
        self.tensors.push(Tensor { name: name.clone(), slots: Vec::new() });
        self.tensor_by_name.insert(name.clone(), id);
        id
    }

    /// The index with this name and prime level, created if missing.
    pub fn index_or_create(&mut self, name: &Name, prime: u32) -> IndexId {
        if let Some(id) = self.find_index(name, prime) {
            return id;
        }
        let id = IndexId(self.indices.len());
        self.indices.push(Index {
            name: name.clone(),
            prime,
            tags: BTreeSet::new(),
            dim: None,
            holders: Vec::new(),
        });
        self.index_by_key.insert((name.clone(), prime), id);
        id
    }

    #[allow(clippy::should_implement_trait)]
    pub fn index_mut(&mut self, id: IndexId) -> &mut Index {
        &mut self.indices[id.0]
    }

    /// Give `tensor` a slot holding `index`.
    pub fn attach(&mut self, tensor: TensorId, index: IndexId, label: Option<String>) -> Result<usize> {
        if let Some(label) = &label
            && self.tensor(tensor).slots.iter().any(|s| s.label.as_ref() == Some(label))
        {
            return Err(Error::new(format!("{} already has a leg `{label}`", self.tensor(tensor).name)));
        }
        if self.index(index).holders.len() >= 2 {
            let i = self.index(index);
            return Err(Error::new(format!(
                "index {} already joins two tensors",
                index_display(&i.name, i.prime)
            )));
        }
        let t = &mut self.tensors[tensor.0];
        let slot = t.slots.len();
        t.slots.push(Slot { index, label });
        self.indices[index.0].holders.push((tensor, slot));
        Ok(slot)
    }

    fn slot_by_label(&self, tensor: TensorId, label: &str) -> Option<usize> {
        self.tensor(tensor).slots.iter().position(|s| s.label.as_deref() == Some(label))
    }

    /// Connect two tensors.  Without leg labels, an existing bond between
    /// them is reused, so `A - B` twice is one bond; labelled legs are
    /// matched by label, and new legs are created as needed.
    pub fn connect(
        &mut self,
        a: TensorId,
        la: Option<&str>,
        b: TensorId,
        lb: Option<&str>,
    ) -> Result<IndexId> {
        let sa = la.and_then(|l| self.slot_by_label(a, l));
        let sb = lb.and_then(|l| self.slot_by_label(b, l));
        match (sa, sb) {
            (Some(sa), Some(sb)) => {
                let (ia, ib) = (self.tensor(a).slots[sa].index, self.tensor(b).slots[sb].index);
                if ia == ib {
                    Ok(ia)
                } else {
                    Err(Error::new(format!(
                        "{}.{} and {}.{} are already used by other bonds",
                        self.tensor(a).name,
                        la.unwrap_or_default(),
                        self.tensor(b).name,
                        lb.unwrap_or_default()
                    )))
                }
            }
            (Some(sa), None) => {
                let index = self.tensor(a).slots[sa].index;
                self.attach(b, index, lb.map(str::to_string))?;
                Ok(index)
            }
            (None, Some(sb)) => {
                let index = self.tensor(b).slots[sb].index;
                self.attach(a, index, la.map(str::to_string))?;
                Ok(index)
            }
            (None, None) => {
                if la.is_none()
                    && lb.is_none()
                    && let Some(existing) = self.bond_between(a, b)
                {
                    return Ok(existing);
                }
                self.auto_links += 1;
                let index = self.index_or_create(&Name::new(AUTO_LINK, [self.auto_links]), 0);
                self.index_mut(index).tags.insert("Link".into());
                self.attach(a, index, la.map(str::to_string))?;
                self.attach(b, index, lb.map(str::to_string))?;
                Ok(index)
            }
        }
    }

    /// An existing bond between two tensors, if any.
    pub fn bond_between(&self, a: TensorId, b: TensorId) -> Option<IndexId> {
        self.tensor(a).slots.iter().map(|s| s.index).find(|&i| {
            let h = self.index(i).holders();
            h.len() == 2 && {
                let (x, y) = (h[0].0, h[1].0);
                (x == a && y == b) || (x == b && y == a)
            }
        })
    }

    /// Give a tensor a new open leg, tagged `Site`.
    pub fn add_open_leg(&mut self, tensor: TensorId, label: Option<String>) -> Result<usize> {
        self.auto_sites += 1;
        let index = self.index_or_create(&Name::new(AUTO_SITE, [self.auto_sites]), 0);
        self.index_mut(index).tags.insert("Site".into());
        self.attach(tensor, index, label)
    }

    pub fn add_group(&mut self, name: &str, members: Vec<TensorId>) -> Result<()> {
        if self.group(name).is_some() {
            return Err(Error::new(format!("group `{name}` is already defined")));
        }
        if self.find_tensor(&Name::plain(name)).is_some() {
            return Err(Error::new(format!("`{name}` is already a tensor name")));
        }
        self.groups.push(Group { name: name.to_string(), members });
        Ok(())
    }

    // ---- Style -----------------------------------------------------------

    /// The attributes of a tensor after the cascade.
    pub fn tensor_style(&self, t: TensorId) -> Vec<Attr> {
        let name = &self.tensor(t).name;
        self.cascade(|sel| match sel {
            Selector::Tensors => Some(0),
            Selector::Tensor(id) => (*id == t).then_some(2),
            Selector::Name(p) => {
                if p.is_bare()
                    && let Some(g) = self.group(&p.base)
                {
                    return g.members.contains(&t).then_some(1);
                }
                p.matches(name, 0).then_some(2)
            }
            _ => None,
        })
    }

    /// The attributes of a bond (an index held by two slots) after the
    /// cascade.
    pub fn bond_style(&self, i: IndexId) -> Vec<Attr> {
        let index = self.index(i);
        self.cascade(|sel| match sel {
            Selector::Bonds => Some(0),
            _ => self.index_rule(sel, i, index),
        })
    }

    /// The attributes of one slot after the cascade.  Rules on the index
    /// apply too when the slot is an open leg.
    pub fn leg_style(&self, t: TensorId, slot: usize) -> Vec<Attr> {
        let tensor = self.tensor(t);
        let s = &tensor.slots[slot];
        let index = self.index(s.index);
        self.cascade(|sel| match sel {
            Selector::Legs => Some(0),
            Selector::OpenLegs => index.is_open().then_some(1),
            Selector::Slot(id, k) => (*id == t && *k == slot).then_some(2),
            Selector::LegOf(p, key) => {
                let hit = p.matches(&tensor.name, 0)
                    && match key {
                        LegKey::Label(l) => s.label.as_deref() == Some(l.as_str()),
                        LegKey::Position(n) => *n == slot + 1,
                    };
                hit.then_some(2)
            }
            _ if index.is_open() => self.index_rule(sel, s.index, index),
            _ => None,
        })
    }

    fn index_rule(&self, sel: &Selector, i: IndexId, index: &Index) -> Option<u8> {
        match sel {
            Selector::Tag(tag) => index.tags.contains(tag).then_some(1),
            Selector::Name(p) => p.matches(&index.name, index.prime).then_some(2),
            Selector::Index(id) => (*id == i).then_some(2),
            _ => None,
        }
    }

    /// Merge the matching rules: by specificity (0 type, 1 tag or group,
    /// 2 name), and in source order within a specificity.
    fn cascade(&self, specificity: impl Fn(&Selector) -> Option<u8>) -> Vec<Attr> {
        let mut hits: Vec<(u8, &Rule)> =
            self.rules.iter().filter_map(|r| specificity(&r.selector).map(|s| (s, r))).collect();
        hits.sort_by_key(|(s, _)| *s);
        merge_attrs(hits.iter().map(|(_, r)| r.attrs.as_slice()))
    }
}

pub(crate) fn index_display(name: &Name, prime: u32) -> String {
    format!("{name}{}", "'".repeat(prime as usize))
}
