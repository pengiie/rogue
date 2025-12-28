pub unsafe fn get_vtable_ptr<DynSrc: ?Sized>(val_dyn: &DynSrc) -> *const () {
    // pointer to the dyn pointer, reinterpreting as a tuple of two pointers.
    let val_dyn_fat_ptr = std::ptr::from_ref(&val_dyn) as *const _ as *const (*const (), *const ());
    return val_dyn_fat_ptr.as_ref().unwrap().1;
}
