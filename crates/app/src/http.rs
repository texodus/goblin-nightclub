//                                    ▗◡≝▔▔` ▔≝=◺▂             ∕        ▐▋  ▝▉
//                                  ╣▒◿▒≘≉◬_    _▒▆▒▀         ◞         ▐▋  ▐▌
//                                  ╔`     ▝◺◿▒▟▘╔╹ ▚◛                  ▐▌  ▐▏
//       G O B L I N               ▕▎_"▃▅≣P ▐▓▛▗▉▂_▂__◚◛_         __▂▂▂▂▟▙◓▀▀▀▀≝
//        N I G H T C L U B         ║`║║_▂▐╝ ▟◙██▛╬▔`_▂◙▒▖       ▕∏▃▃▃▃▂▐▌║▂▂▂▔▔
//                                  ▙⋡▜╥∺╝ ▗▟███▛Ƒ  ▐▛▔▔▜█▌      ▐╣ ▝▜███▋ ∭▔▔▔▔
//                                   ▁⋡▚▃▄▀▉Ƣ█▀▉╺▗▐∏▊   ╠█▎      ▐╣   ▁∭█▉ ▊ ∭▐█
//                          ▁▃◛◭◒≠◚▔▔╸ ╔▒∏▕▓▌ ▟▎ ▐▒██▖    ▜▎     ╠▎≕∏__  ◙∏▍ ╣█▌
//                     _◿▎`▂_ ▟▒║╣▂▂▂▂▞╜▔▜ ╬◙▏∏▎ ▐███◘⊠◚"` ▜     ╣ ▂▅▅▅_ ▐█▎▕▒█Ǩ
//              _▂_ _▂▂▒∭▒▒◛≝▔           ▕▶╣◚▔▔▔╠∏∏║▌ ┉━━≝▒▃▘   ▕▌▟█████▙ ∭ ▐▐▌▓
//    ▁▁▁▁_╓◛▋▝◠▬Ǟ◡▃╠◚≝≊■■■▆▆▤▀◚⊜⊛╥▄▃▃▂_║╳▞▗╝▟▛◚███████████▌    ▕▒████◚◚█▌▋ ∭▟▎╱
//   ▐▔▔"⋨∭◅▃╏◠≉╬▒▇▆▅┫ ▕▄▃∏◠━━◡▃▂║▃▃▃▄▙▄∭▟▟▙▓≝▬▬╩◘██████████    ▐∏██◘▜▁╔▕▒▎ ▒█▎╣
//   ▕╣◚◘▉▐▄▃▂▁▟▒╝◙▉◙Ů▒▒◚▜▛◙◘▒▒▌▍ ▔▔"▀▬ǡ∗▃∑◚▀▀◘◘▓████▀╣▟╢▒◙◙▙▖  ▐ ╬     ∭▐▏▐█▉▏▜
//    ĴĄ  ╲╲ ▔╠▛◘◘▓▍▅╣┇▐▒∭Ǭ▛╨◀∗▃▒▀Ň◘█▒⋯┳▂▃_▐█████████████████▌∑◚╣__%◺_⊋∛ ∭ ▐▐▉▌▝
//  ▔  ┗▌   ▔◚▜▍▝▀▀▜█▌▐▐▐██▙▄▃▂_ ▗╢▒▛◚▬∗▃▂◚▀╩╩╩◘◘◙╩◘Ů▔▔▔▔▔▔▔▔▔▝▜▒▒▒▒▒◘◘▣▤▌ ∭█▌▕▎
//  Copyright 2026, [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0).

//! A packed level's bytes from a server, by range requests (see
//! [`goblin_nightclub_core::packed::Bytes`]): each range asked for is
//! fetched in the background, and there for the next ask.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use goblin_nightclub_core::packed::Bytes;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, Response};

/// Each range asked for, from and how long: its bytes once they've come,
/// `None` until then.
type Ranges = HashMap<(u64, usize), Option<Arc<[u8]>>>;

/// A handle on the bytes at a URL; clones share what's been fetched.
#[derive(Clone)]
pub struct HttpBytes {
    url: Rc<str>,
    ranges: Rc<RefCell<Ranges>>,
}

impl HttpBytes {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.into(),
            ranges: Default::default(),
        }
    }
}

impl Bytes for HttpBytes {
    fn read(&self, at: u64, len: usize) -> Option<Arc<[u8]>> {
        if len == 0 {
            return Some(Arc::default());
        }
        if let Some(got) = self.ranges.borrow().get(&(at, len)) {
            return got.clone();
        }
        self.ranges.borrow_mut().insert((at, len), None);
        let (url, ranges) = (self.url.clone(), self.ranges.clone());
        wasm_bindgen_futures::spawn_local(async move {
            // Whatever goes wrong, what comes is short: the reader says so.
            let bytes = fetch(&url, at, len).await.unwrap_or_else(|e| {
                web_sys::console::error_2(&format!("fetching {url}:").into(), &e);
                Vec::new()
            });
            ranges.borrow_mut().insert((at, len), Some(bytes.into()));
        });
        None
    }
}

/// `len` bytes of `url` from `at`.
async fn fetch(url: &str, at: u64, len: usize) -> Result<Vec<u8>, JsValue> {
    let headers = Headers::new()?;
    headers.set("Range", &format!("bytes={at}-{}", at + len as u64 - 1))?;
    let init = RequestInit::new();
    init.set_method("GET");
    init.set_headers(&headers);
    let request = Request::new_with_str_and_init(url, &init)?;
    let window = web_sys::window().ok_or("no window")?;
    let response: Response = JsFuture::from(window.fetch_with_request(&request))
        .await?
        .dyn_into()?;
    if !response.ok() {
        return Err(format!("{} {}", response.status(), response.status_text()).into());
    }
    let body = JsFuture::from(response.array_buffer()?).await?;
    let bytes = js_sys::Uint8Array::new(&body).to_vec();
    // A server that doesn't do ranges sends the lot.
    if response.status() == 200 {
        let at = at as usize;
        return Ok(bytes.get(at..at + len).unwrap_or_default().to_vec());
    }
    Ok(bytes)
}
