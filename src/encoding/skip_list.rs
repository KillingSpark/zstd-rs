use core::{
    borrow::Borrow,
    cell::{Cell, RefCell},
    ops::Deref,
};
use std::{rc::Rc, vec::Vec};

pub(crate) struct Skiplist<T> {
    layers: Vec<Rc<RefCell<Inner<T>>>>,
    bottom: Option<Rc<Leaf<T>>>,
}

struct Inner<T> {
    next: Option<Rc<RefCell<Inner<T>>>>,
    lower_layer: InnerTarget<T>,
    element: T,
}

enum InnerTarget<T> {
    Leaf(Rc<Leaf<T>>),
    Inner(Rc<RefCell<Inner<T>>>),
}

struct Leaf<T> {
    next: RefCell<Option<Rc<Leaf<T>>>>,
    element: T,
}

impl<T: Ord + Eq + Copy> Skiplist<T> {
    pub(crate) fn add(&mut self, element: T) -> (Option<T>, Option<T>) {
        if self.bottom.is_none() {
            self.bottom = Some(Rc::new(Leaf {
                element,
                next: RefCell::new(None),
            }));
            return (None, None);
        }

        let mut path_start = None;
        for layer in self.layers.iter() {
            if RefCell::borrow(layer).element > element {
                continue;
            }
            path_start = Some(Rc::clone(layer));
        }

        let leaf_start: Rc<Leaf<T>>;
        let mut path: Vec<Rc<RefCell<Inner<T>>>> = Vec::new();
        if let Some(mut inner) = path_start {
            path.push(Rc::clone(&inner));
            loop {
                let inner_ref = RefCell::borrow(&inner);
                if inner_ref.element < element {
                    if let Some(next) = inner_ref.next.as_ref() {
                        if RefCell::borrow(next).element < element {
                            let next = Rc::clone(next);
                            drop(inner_ref);
                            inner = next;
                            continue;
                        }
                    }
                }
                match inner_ref.lower_layer {
                    InnerTarget::Inner(ref target) => {
                        let next = Rc::clone(target);
                        drop(inner_ref);
                        inner = next;
                        continue;
                    }
                    InnerTarget::Leaf(ref leaf) => {
                        leaf_start = Rc::clone(leaf);
                        break;
                    }
                }
            }
        } else {
            leaf_start = Rc::clone(self.bottom.as_ref().unwrap());
        }

        let mut leaf = leaf_start;
        loop {
            let leaf_ref = leaf.next.borrow();
            if let Some(ref next) = leaf_ref.as_ref() {
                if next.element < element {
                    let next_leaf = Rc::clone(&next.borrow());
                    drop(leaf_ref);
                    leaf = next_leaf;
                    continue;
                }
            }
            break;
        }

        let previous = leaf;
        let after = previous.next.borrow().as_ref().map(|next| Rc::clone(next));

        let after_return = after.borrow().as_ref().map(|after| after.element);

        let new_leaf = Rc::new(Leaf {
            next: RefCell::new(after),
            element,
        });

        // maybe add new inner nodes as next to the inner nodes in path

        *previous.next.borrow_mut() = Some(new_leaf);

        (Some(previous.element), after_return)
    }
}
