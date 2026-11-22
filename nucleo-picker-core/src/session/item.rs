use crate::Generation;

/// An opaque item identifier which uniquely identifies an item `T` within its originating picker.
///
/// It is guaranteed that if two [`ItemId`]s are equal then the underlying items are also equal
/// (i.e., they represent the same memory location of the underlying item `&T`). In particular, this
/// contract is upheld across restarts and across different pickers.
///
/// The contract is upheld by internally storing the [generation](Generation) of the underlying item
/// pool. Read those docs for more details. Equality of generation does not imply anything about
/// equality of items.
///
/// Within a single generation, the order corresponds to the order in which items were received by
/// the picker. Order across generations is unspecified.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId {
    pub(crate) generation: Generation,
    pub(crate) index: u32,
}

impl ItemId {
    /// Returns the generation of the underlying item pool which contains this item.
    #[must_use]
    #[inline]
    pub const fn generation(self) -> Generation {
        self.generation
    }
}

/// A picker item along with its identifier.
#[derive(Debug)]
pub struct ItemRef<'a, T> {
    id: ItemId,
    value: &'a T,
}

impl<T> Copy for ItemRef<'_, T> {}

impl<T> Clone for ItemRef<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, T> ItemRef<'a, T> {
    #[inline]
    pub(crate) fn new(id: ItemId, value: &'a T) -> Self {
        Self { id, value }
    }

    /// The identifier for this item in the originating picker.
    #[inline]
    pub fn id(&self) -> ItemId {
        self.id
    }

    /// The borrowed item.
    #[inline]
    pub fn value(&self) -> &'a T {
        self.value
    }
}

/// An opaque handle to an item in the picker.
///
/// This item persists independently of the picker, including across restart. Holding this handle
/// will prevent the underlying item pool from being deallocated.
pub struct OwnedItem<T> {
    id: ItemId,
    item: nucleo::DetachedItem<T>,
}

impl<T> OwnedItem<T> {
    #[inline]
    pub(super) fn new(id: ItemId, item: nucleo::DetachedItem<T>) -> Self {
        Self { id, item }
    }

    /// Returns the underlying item's original idenitifier.
    #[inline]
    pub fn id(&self) -> ItemId {
        self.id
    }

    /// Returns the underlying item's generation.
    #[inline]
    pub fn generation(&self) -> Generation {
        self.id.generation()
    }

    /// Borrow the underlying item.
    #[inline]
    pub fn value(&self) -> &T {
        self.item.item().data
    }

    /// Returns a reference to the item.
    #[inline]
    pub fn item_ref(&self) -> ItemRef<'_, T> {
        ItemRef::new(self.id(), self.value())
    }
}
