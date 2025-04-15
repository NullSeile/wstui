pub trait CallbackTranslator {
    type CType;
    type RustType;

    unsafe fn to_rust(c_value: Self::CType) -> Self::RustType;

    unsafe fn invoke_closure(
        closure: &mut Box<dyn FnMut(Self::RustType)>,
        rust_value: Self::RustType,
    );
}

macro_rules! define_callback {
    ($callback_name:ident, $c_func:ident, $translator:ty) => {
        fn $callback_name<F>(callback: F)
        // fn $callback_name<F>(mut callback: F)
        where
            F: FnMut(<$translator as CallbackTranslator>::RustType) + 'static,
        {
            // Double-box the closure to erase its type
            let callback: Box<dyn FnMut(_)> = Box::new(callback);
            let callback_box = Box::new(Arc::new(Mutex::new(callback)));
            let user_data = Box::into_raw(callback_box) as *mut c_void;

            // Shim callback compatible with C
            extern "C" fn shim(
                c_param: <$translator as CallbackTranslator>::CType,
                user_data: *mut c_void,
            ) {
                unsafe {
                    let callback_box =
                        Box::from_raw(user_data as *mut Arc<Mutex<Box<dyn FnMut(_)>>>);
                    let closure = Arc::clone(&callback_box);
                    let mut guard = closure.lock().unwrap();

                    // Convert C type to Rust type
                    let rust_value = <$translator as CallbackTranslator>::to_rust(c_param);

                    // Invoke the user's closure
                    <$translator as CallbackTranslator>::invoke_closure(&mut guard, rust_value);

                    // TODO: Make sure this works
                    let _ = Box::into_raw(callback_box);
                }
            }

            unsafe {
                // Call the C function (e.g., forEachContact)
                $c_func(shim, user_data);
            }
        }
    };
}
