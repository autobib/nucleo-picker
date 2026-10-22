/// An opaque item identifier which uniquely identifies an item `T` within its originating picker.
///
/// It is guaranteed that if two [`ItemId`]s obtained *from the same picker* are equal, then the
/// corresponding items are also identical (i.e., they represent the same memory location of the underlying
/// item `&T`). In particular, this contract is upheld across restarts.
///
/// However, *different pickers may reuse identifiers*. Therefore, identifiers originating from
/// different pickers *may be the same, even if the underlying item is different*. Use caution! If
/// you require a more robust identification scheme, you must store this identity inside the item
/// itself.
///
/// Note that ordering has no relationship to ranking or the order in which items were received by
/// the picker; it is only provided for convenience of use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId {
    pub(crate) generation: u32,
    pub(crate) index: u32,
}

/// A picker item along with its identifier.
///
/// The identifier is scoped to the originating picker and is not meaningful when compared
/// between different pickers. See [`ItemId`] for more details.
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
    pub(crate) fn new(id: ItemId, value: &'a T) -> Self {
        Self { id, value }
    }

    /// The identifier for this item in the originating picker.
    pub fn id(&self) -> ItemId {
        self.id
    }

    /// The borrowed item.
    pub fn value(&self) -> &'a T {
        self.value
    }
}
