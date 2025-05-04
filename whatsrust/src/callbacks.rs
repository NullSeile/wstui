pub trait CallbackTranslator<T> {
    unsafe fn to_rust(c_value: T) -> Self;
}

// impl<T, U: From<T>> CallbackTranslator<T> for U {
//     unsafe fn to_rust(c_value: T) -> Self {
//         Self::from(c_value)
//     }
// }

macro_rules! setup_handler {
    ($fn_name:ident, $c_func:ident) => {
        callback_v2!($fn_name, $c_func,);
    };
    ($fn_name:ident, $c_func:ident, $($c_type:ty => $rs_type:ty),*) => {
        pub fn $fn_name<F>(callback: F)
        where
            F: FnMut($($rs_type),*) + 'static,
        {
            // Double-box the closure to erase its type
            type CallbackType = dyn FnMut($($rs_type),*);
            let callback: Box<CallbackType> = Box::new(callback);
            let callback_box = Box::new(Arc::new(Mutex::new(callback)));
            let user_data = Box::into_raw(callback_box) as *mut c_void;

            // Shim callback compatible with C
            extern "C" fn shim(
                $(
                    __param: $c_type,
                )*
                user_data: *mut c_void
            ) {
                unsafe {
                    let callback_box =
                        Box::from_raw(user_data as *mut Arc<Mutex<Box<CallbackType>>>);
                    let closure = Arc::clone(&callback_box);
                    let mut guard = closure.lock().unwrap();

                    guard($(
                        <$rs_type>::to_rust(__param),
                    ),*);

                    // TODO: Make sure this works
                    let _ = Box::into_raw(callback_box);
                }
            }

            unsafe {
                $c_func(shim, user_data);
            }
        }
    };
}
