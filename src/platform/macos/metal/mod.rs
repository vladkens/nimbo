// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Metal renderer, built with the `metal` feature and chosen per window with
//! `NIMBO_RENDERER=metal`: the scene drawn on the GPU into the same IOSurfaces the CPU renderer
//! fills. Its shaders are the twin of [crate::cpu::raster], function by function, so both draw
//! the same pixels; the CPU renderer stays the default and the reference.
