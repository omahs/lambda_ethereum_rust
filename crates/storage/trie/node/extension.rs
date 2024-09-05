use crate::error::StoreError;
use crate::trie::db::TrieDB;
use crate::trie::nibble::NibbleSlice;
use crate::trie::ValueRLP;
use crate::trie::{nibble::NibbleVec, node_ref::NodeRef};

use crate::trie::hashing::{NodeHash, NodeHashRef, NodeHasher, PathKind};

use super::{BranchNode, LeafNode, Node};

#[derive(Debug)]
pub struct ExtensionNode {
    pub hash: NodeHash,
    pub prefix: NibbleVec,
    pub child: NodeRef,
}

impl ExtensionNode {
    pub(crate) fn new(prefix: NibbleVec, child: NodeRef) -> Self {
        Self {
            prefix,
            child,
            hash: Default::default(),
        }
    }
    pub fn get(&self, db: &TrieDB, mut path: NibbleSlice) -> Result<Option<ValueRLP>, StoreError> {
        // If the path is prefixed by this node's prefix, delegate to its child.
        // Otherwise, no value is present.
        if path.skip_prefix(&self.prefix) {
            let child_node = db
                .get_node(self.child)?
                .expect("inconsistent internal tree structure");

            child_node.get(db, path)
        } else {
            Ok(None)
        }
    }

    pub fn insert(
        mut self,
        db: &mut TrieDB,
        mut path: NibbleSlice,
        value: ValueRLP,
    ) -> Result<Node, StoreError> {
        // Possible flow paths (there are duplicates between different prefix lengths):
        //   extension { [0], child } -> branch { 0 => child } with_value !
        //   extension { [0], child } -> extension { [0], child }
        //   extension { [0, 1], child } -> branch { 0 => extension { [1], child } } with_value !
        //   extension { [0, 1], child } -> extension { [0], branch { 1 => child } with_value ! }
        //   extension { [0, 1], child } -> extension { [0, 1], child }
        //   extension { [0, 1, 2], child } -> branch { 0 => extension { [1, 2], child } } with_value !
        //   extension { [0, 1, 2], child } -> extension { [0], branch { 1 => extension { [2], child } } with_value ! }
        //   extension { [0, 1, 2], child } -> extension { [0, 1], branch { 2 => child } with_value ! }
        //   extension { [0, 1, 2], child } -> extension { [0, 1, 2], child }

        self.hash.mark_as_dirty();

        if path.skip_prefix(&self.prefix) {
            // [Note]: Original impl would remove
            let child_node = db
                .get_node(self.child)?
                .expect("inconsistent internal tree structure");

            let child_node = child_node.insert(db, path, value)?;
            self.child = db.insert_node(child_node)?;

            Ok(self.into())
        } else {
            let offset = path.clone().count_prefix_vec(&self.prefix);
            path.offset_add(offset);
            let (left_prefix, choice, right_prefix) = self.prefix.split_extract_at(offset);

            let left_prefix = (!left_prefix.is_empty()).then_some(left_prefix);
            let right_prefix = (!right_prefix.is_empty()).then_some(right_prefix);

            // Prefix right node (if any, child is self.child_ref).
            let right_prefix_node = if let Some(right_prefix) = right_prefix {
                db.insert_node(ExtensionNode::new(right_prefix, self.child).into())?
            } else {
                self.child
            };

            // Branch node (child is prefix right or self.child_ref).
            let mut choices = [Default::default(); 16];
            choices[choice as usize] = right_prefix_node;
            let branch_node = if let Some(c) = path.next() {
                let new_leaf = LeafNode::new(path.data(), value);
                choices[c as usize] = db.insert_node(new_leaf.into())?;
                BranchNode::new(choices)
            } else {
                BranchNode::new_with_value(choices, path.data(), value)
            };

            // Prefix left node (if any, child is branch_node).
            match left_prefix {
                Some(left_prefix) => {
                    let branch_ref = db.insert_node(branch_node.into())?;

                    Ok(ExtensionNode::new(left_prefix, branch_ref).into())
                }
                None => Ok(branch_node.into()),
            }
        }
    }

    pub fn remove(
        mut self,
        db: &mut TrieDB,
        mut path: NibbleSlice,
    ) -> Result<(Option<Node>, Option<ValueRLP>), StoreError> {
        // Possible flow paths:
        //   - extension { a, branch { ... } } -> extension { a, branch { ... }}
        //   - extension { a, branch { ... } } -> extension { a + b, branch { ... }}
        //   - extension { a, branch { ... } } -> leaf { ... }

        if path.skip_prefix(&self.prefix) {
            let child_node = db
                //[NOTE] reference impl would remove
                .get_node(self.child)?
                .expect("inconsistent internal tree structure");

            let (child_node, old_value) = child_node.remove(db, path)?;
            if old_value.is_some() {
                self.hash.mark_as_dirty();
            }
            let node = match child_node {
                None => None,
                Some(node) => Some(match node {
                    Node::Branch(branch_node) => {
                        self.child = db.insert_node(branch_node.into())?;
                        self.into()
                    }
                    Node::Extension(mut extension_node) => {
                        self.prefix.extend(&extension_node.prefix);
                        extension_node.prefix = self.prefix;
                        extension_node.into()
                    }
                    Node::Leaf(leaf_node) => leaf_node.into(),
                }),
            };

            Ok((node, old_value))
        } else {
            Ok((Some(self.into()), None))
        }
    }

    pub fn compute_hash(&self, db: &TrieDB, path_offset: usize) -> Result<NodeHashRef, StoreError> {
        if let Some(hash) = self.hash.extract_ref() {
            return Ok(hash);
        };
        let child_node = db
            .get_node(self.child)?
            .expect("inconsistent internal tree structure");

        let child_hash_ref = child_node.compute_hash(db, path_offset + self.prefix.len())?;

        Ok(compute_extension_hash(
            &self.hash,
            &self.prefix,
            child_hash_ref,
        ))
    }
}

pub fn compute_extension_hash<'a>(
    hash: &'a NodeHash,
    prefix: &NibbleVec,
    child_hash_ref: NodeHashRef,
) -> NodeHashRef<'a> {
    let prefix_len = NodeHasher::path_len(prefix.len());
    let child_len = match &child_hash_ref {
        NodeHashRef::Inline(x) => x.len(),
        NodeHashRef::Hashed(x) => NodeHasher::bytes_len(x.len(), x[0]),
    };

    let mut hasher = NodeHasher::new(hash);
    hasher.write_list_header(prefix_len + child_len);
    hasher.write_path_vec(prefix, PathKind::Extension);
    match child_hash_ref {
        NodeHashRef::Inline(x) => hasher.write_raw(&x),
        NodeHashRef::Hashed(x) => hasher.write_bytes(&x),
    }
    hasher.finalize()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        pmt_node,
        trie::{nibble::Nibble, Trie},
    };

    #[test]
    fn new() {
        let node = ExtensionNode::new(NibbleVec::new(), Default::default());

        assert_eq!(node.prefix.len(), 0);
        assert_eq!(node.child, NodeRef::default());
    }

    #[test]
    fn get_some() {
        let mut trie = Trie::new_temp();
        let node = pmt_node! { @(trie)
            extension { [0], branch {
                0 => leaf { vec![0x00] => vec![0x12, 0x34, 0x56, 0x78] },
                1 => leaf { vec![0x01] => vec![0x34, 0x56, 0x78, 0x9A] },
            } }
        };

        assert_eq!(
            node.get(&trie.db, NibbleSlice::new(&[0x00])).unwrap(),
            Some(vec![0x12, 0x34, 0x56, 0x78]),
        );
        assert_eq!(
            node.get(&trie.db, NibbleSlice::new(&[0x01])).unwrap(),
            Some(vec![0x34, 0x56, 0x78, 0x9A]),
        );
    }

    #[test]
    fn get_none() {
        let mut trie = Trie::new_temp();
        let node = pmt_node! { @(trie)
            extension { [0], branch {
                0 => leaf { vec![0x00] => vec![0x12, 0x34, 0x56, 0x78] },
                1 => leaf { vec![0x01] => vec![0x34, 0x56, 0x78, 0x9A] },
            } }
        };

        assert_eq!(node.get(&trie.db, NibbleSlice::new(&[0x02])).unwrap(), None,);
    }

    #[test]
    fn insert_passthrough() {
        let mut trie = Trie::new_temp();
        let node = pmt_node! { @(trie)
            extension { [0], branch {
                0 => leaf { vec![0x00] => vec![0x12, 0x34, 0x56, 0x78] },
                1 => leaf { vec![0x01] => vec![0x34, 0x56, 0x78, 0x9A] },
            } }
        };

        let node = node
            .insert(&mut trie.db, NibbleSlice::new(&[0x02]), vec![])
            .unwrap();
        let node = match node {
            Node::Extension(x) => x,
            _ => panic!("expected an extension node"),
        };
        assert!(node.prefix.iter().eq([Nibble::V0].into_iter()));
    }

    #[test]
    fn insert_branch() {
        let mut trie = Trie::new_temp();
        let node = pmt_node! { @(trie)
            extension { [0], branch {
                0 => leaf { vec![0x00] => vec![0x12, 0x34, 0x56, 0x78] },
                1 => leaf { vec![0x01] => vec![0x34, 0x56, 0x78, 0x9A] },
            } }
        };

        let node = node
            .insert(&mut trie.db, NibbleSlice::new(&[0x10]), vec![])
            .unwrap();
        let node = match node {
            Node::Branch(x) => x,
            _ => panic!("expected a branch node"),
        };
        assert!(node.choices.iter().any(|x| &x == &&NodeRef::new(3)));
    }

    #[test]
    fn insert_branch_extension() {
        let mut trie = Trie::new_temp();
        let node = pmt_node! { @(trie)
            extension { [0, 0], branch {
                0 => leaf { vec![0x00, 0x00] => vec![0x12, 0x34, 0x56, 0x78] },
                1 => leaf { vec![0x00, 0x10] => vec![0x34, 0x56, 0x78, 0x9A] },
            } }
        };

        let node = node
            .insert(&mut trie.db, NibbleSlice::new(&[0x10]), vec![])
            .unwrap();
        let node = match node {
            Node::Branch(x) => x,
            _ => panic!("expected a branch node"),
        };
        assert!(node.choices.iter().any(|x| &x == &&NodeRef::new(4)));
    }

    #[test]
    fn insert_extension_branch() {
        let mut trie = Trie::new_temp();
        let node = pmt_node! { @(trie)
            extension { [0, 0], branch {
                0 => leaf { vec![0x00, 0x00] => vec![0x12, 0x34, 0x56, 0x78] },
                1 => leaf { vec![0x00, 0x10] => vec![0x34, 0x56, 0x78, 0x9A] },
            } }
        };

        let path = NibbleSlice::new(&[0x01]);
        let value = vec![0x02];

        let node = node
            .insert(&mut trie.db, path.clone(), value.clone())
            .unwrap();

        assert!(matches!(node, Node::Extension(_)));
        assert_eq!(node.get(&mut trie.db, path).unwrap(), Some(value));
    }

    #[test]
    fn insert_extension_branch_extension() {
        let mut trie = Trie::new_temp();
        let node = pmt_node! { @(trie)
            extension { [0, 0], branch {
                0 => leaf { vec![0x00, 0x00] => vec![0x12, 0x34, 0x56, 0x78] },
                1 => leaf { vec![0x00, 0x10] => vec![0x34, 0x56, 0x78, 0x9A] },
            } }
        };

        let path = NibbleSlice::new(&[0x01]);
        let value = vec![0x04];

        let node = node
            .insert(&mut trie.db, path.clone(), value.clone())
            .unwrap();

        assert!(matches!(node, Node::Extension(_)));
        assert_eq!(node.get(&mut trie.db, path).unwrap(), Some(value));
    }

    #[test]
    fn remove_none() {
        let mut trie = Trie::new_temp();
        let node = pmt_node! { @(trie)
            extension { [0], branch {
                0 => leaf { vec![0x00] => vec![0x00] },
                1 => leaf { vec![0x01] => vec![0x01] },
            } }
        };

        let (node, value) = node
            .remove(&mut trie.db, NibbleSlice::new(&[0x02]))
            .unwrap();

        assert!(matches!(node, Some(Node::Extension(_))));
        assert_eq!(value, None);
    }

    #[test]
    fn remove_into_leaf() {
        let mut trie = Trie::new_temp();
        let node = pmt_node! { @(trie)
            extension { [0], branch {
                0 => leaf { vec![0x00] => vec![0x00] },
                1 => leaf { vec![0x01] => vec![0x01] },
            } }
        };

        let (node, value) = node
            .remove(&mut trie.db, NibbleSlice::new(&[0x01]))
            .unwrap();

        assert!(matches!(node, Some(Node::Leaf(_))));
        assert_eq!(value, Some(vec![0x01]));
    }

    #[test]
    fn remove_into_extension() {
        let mut trie = Trie::new_temp();
        let node = pmt_node! { @(trie)
            extension { [0], branch {
                0 => leaf { vec![0x00] => vec![0x00] },
                1 => extension { [0], branch {
                    0 => leaf { vec![0x01, 0x00] => vec![0x01, 0x00] },
                    1 => leaf { vec![0x01, 0x01] => vec![0x01, 0x01] },
                } },
            } }
        };

        let (node, value) = node
            .remove(&mut trie.db, NibbleSlice::new(&[0x00]))
            .unwrap();

        assert!(matches!(node, Some(Node::Extension(_))));
        assert_eq!(value, Some(vec![0x00]));
    }

    #[test]
    fn compute_hash() {
        let mut trie = Trie::new_temp();
        let node = pmt_node! { @(trie)
            extension { [0, 0], branch {
                0 => leaf { vec![0x00, 0x00] => vec![0x12, 0x34] },
                1 => leaf { vec![0x00, 0x10] => vec![0x56, 0x78] },
            } }
        };

        let node_hash_ref = node.compute_hash(&trie.db, 0).unwrap();
        assert_eq!(
            node_hash_ref.as_ref(),
            &[
                0xDD, 0x82, 0x00, 0x00, 0xD9, 0xC4, 0x30, 0x82, 0x12, 0x34, 0xC4, 0x30, 0x82, 0x56,
                0x78, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80,
                0x80, 0x80,
            ],
        );
    }

    #[test]
    fn compute_hash_long() {
        let mut trie = Trie::new_temp();
        let node = pmt_node! { @(trie)
            extension { [0, 0], branch {
                0 => leaf { vec![0x00, 0x00] => vec![0x12, 0x34, 0x56, 0x78, 0x9A] },
                1 => leaf { vec![0x00, 0x10] => vec![0x34, 0x56, 0x78, 0x9A, 0xBC] },
            } }
        };

        let node_hash_ref = node.compute_hash(&trie.db, 0).unwrap();
        assert_eq!(
            node_hash_ref.as_ref(),
            &[
                0xFA, 0xBA, 0x42, 0x79, 0xB3, 0x9B, 0xCD, 0xEB, 0x7C, 0x53, 0x0F, 0xD7, 0x6E, 0x5A,
                0xA3, 0x48, 0xD3, 0x30, 0x76, 0x26, 0x14, 0x84, 0x55, 0xA0, 0xAE, 0xFE, 0x0F, 0x52,
                0x89, 0x5F, 0x36, 0x06,
            ],
        );
    }
}
