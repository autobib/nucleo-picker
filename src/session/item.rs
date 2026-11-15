/// An opaque item identifier which uniquely identifies an item `T` within its originating picker.
///
/// It is guaranteed that if two [`ItemId`]s are equal then the underlying items are also equal
/// (i.e., they represent the same memory location of the underlying item `&T`). In particular, this
/// contract is upheld across restarts and across different pickers using a small amount of atomic
/// global state.
///
/// Note that ordering has no relationship to ranking or the order in which items were received by
/// the picker; it is only provided for convenience of use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId {
    pub(crate) generation: u32,
    pub(crate) index: u32,
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

impl<T: Send + Sync + 'static> OwnedItem<T> {
    #[inline]
    pub(super) fn new(id: ItemId, item: nucleo::DetachedItem<T>) -> Self {
        Self { id, item }
    }

    /// Returns a reference to the item.
    #[inline]
    pub fn item_ref(&self) -> ItemRef<'_, T> {
        ItemRef::new(self.id, self.item.item().data)
    }
}
