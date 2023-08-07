use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    fn setInterval(closure: &Closure<dyn FnMut()>, millis: u32) -> f64;
    fn cancelInterval(token: f64);
}

#[wasm_bindgen]
#[derive(Debug)]
pub struct LeakyInterval {
    token: f64,
}

impl LeakyInterval {
    pub fn new<F: 'static>(duration: std::time::Duration, f: F) -> LeakyInterval
    where
        F: FnMut(),
    {
        let closure = { Closure::wrap(Box::new(f) as Box<dyn FnMut()>) };
        let millis = duration.as_millis() as u32;

        #[allow(unused_unsafe)]
        let token = unsafe { setInterval(&closure, millis) };
        closure.forget();

        LeakyInterval { token }
    }
}

impl Drop for LeakyInterval {
    fn drop(&mut self) {
        #[allow(unused_unsafe)]
        unsafe {
            cancelInterval(self.token);
        }
    }
}
