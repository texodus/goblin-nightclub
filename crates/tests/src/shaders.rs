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

use goblin_nightclub::shaders::{self, CompositeParams, Transform};
use wgpu::naga;

/// `source`, parsed and validated.
fn module(name: &str, source: &str) -> naga::Module {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(source)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(source)));
    module
}

/// The renderer's shaders, as it compiles them, parse and validate: the
/// browser checks them only at runtime, and the renderer is built only
/// for wasm32.
#[test]
fn shaders_are_valid() {
    module("shapes", &shaders::shapes());
    module("composite", &shaders::composite());
}

/// Struct `name` of `module`: its size, and each member's name and offset.
fn layout(module: &naga::Module, name: &str) -> (usize, Vec<(String, usize)>) {
    let (_, ty) = (module.types.iter())
        .find(|(_, t)| t.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no struct {name}"));
    let naga::TypeInner::Struct { members, span } = &ty.inner else {
        panic!("{name} isn't a struct");
    };
    let members = members
        .iter()
        .map(|m| (m.name.clone().unwrap(), m.offset as usize))
        .collect();
    (*span as usize, members)
}

/// Rust struct `$rust`'s layout, as [`layout`] has a shader's: its fields
/// named in order.
macro_rules! rust_layout {
    ($rust:ty, [$($field:ident),* $(,)?]) => {
        (
            size_of::<$rust>(),
            vec![$((stringify!($field).to_string(), std::mem::offset_of!($rust, $field))),*],
        )
    };
}

/// The uniforms the renderer writes are laid out as the shaders read them:
/// the same members, at the same offsets, and the same size.
#[test]
fn uniforms_match_the_shaders() {
    let shapes = module("shapes", &shaders::shapes());
    let transform = rust_layout!(
        Transform,
        [screen, camera, center, scale, zoom, cell, parallax, offset]
    );
    assert_eq!(transform, layout(&shapes, "Globals"));
    let composite = module("composite", &shaders::composite());
    let params = rust_layout!(
        CompositeParams,
        [
            origin,
            radius,
            time,
            top,
            present,
            scale,
            radius_step,
            camera,
            radius_min,
            zoom,
            seen_zoom,
            outdoors,
            ground,
            radius_underground,
            page_px,
            base,
            offset,
            fade,
            sink,
            world,
            sight,
            _pad,
            _pad2,
        ]
    );
    assert_eq!(params, layout(&composite, "Params"));
}
