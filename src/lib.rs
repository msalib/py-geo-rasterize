//#![doc = include_str!("../README.md")]
use std::{convert::TryInto, io::Cursor};

use anyhow::Result;
use geo_rasterize::{BinaryBuilder, LabelBuilder, MergeAlgorithm, Transform};
use ndarray::Array2;
use numpy::{DataType, Element, IntoPyArray, PyArray2, PyArrayDescr};
use pyo3::{
    exceptions::{PyTypeError, PyValueError},
    prelude::*,
    types::{PyBool, PyBytes, PySequence},
};

use geo_types::Geometry;

macro_rules! handle_type {
    ($T:ty, $py:ident, $most:ident, $background:ident, $foregrounds:ident) => {{
        let $background = $background.extract::<$T>()?;
        let $foregrounds = $foregrounds.downcast::<PyArray2<$T>>()?.to_vec()?;
        // this is a bit delicate: for each type, we need to run
        // `generic_rasterize` in the `allow_threads` block, but that will
        // return an `Array2<T>` which we have to convert into a
        // generic `PyAny`, but that conversion requires `py`
        $py.allow_threads(move || {
            const ZERO: $T = 0 as $T;
            const ONE: $T = 1 as $T;
            if ($background == ZERO)
                && ($most.algorithm == MergeAlgorithm::Replace)
                && $foregrounds.iter().all(|&fore| fore == ONE)
            {
                Ok(binary_rasterize($most)?.mapv(|v| if v { ONE } else { ZERO }))
            } else {
                generic_rasterize::<$T>($most, $background, $foregrounds)
            }
        })?
        .into_pyarray($py)
        .as_ref()
    }};
}

/// Burn vector shapes into a raster and return it.
///
/// Parameters
/// ----------
///
/// * shapes: a sequence of `shapely` geometries like Points,
///   Polygons, etc. Invalid shapes won't be rasterized.
///
/// * foregrounds: a sequence of pixel values that will be burned in
///   for each shape; it should have the same length as `shapes`.
///
/// * output_shape: the size of the raster that will be constructed
///   and returned.
///
/// * background: the value that the raster will be initialized with
///   before rasterization or `0` if no value is provided.
///
/// * dtype: specifies the `numpy.dtyp` of the raster array returned;
///   this value can be either a string ('float32') or something like
///   `numpy.float32` or an instance of `numpy.dtype`. When it is not
///   provided, a dtype will be inferred based on the value of
///   `foregrounds` and `background`. Note that if you just use plain
///   python integers, you'll get `int64` as the dtype.
///
/// * algorithm: how to handle cases where multiple shapes touch the
///   same pixel. Choices are either 'replace' (the default) or 'add',
///   which can be used to make heatmaps indicating shape density.
///
/// * geo_to_pix: an affine transform that converts coordinates from
///   world space to pixel space. This must be a 6-element sequence of
///   floats (or numpy array). If you have an `Affine` instance from
///   the `affine` package, you can call `.to_gdal()` to get an
///   appropriate array. Note that usually geo-referenced raster file
///   formats like GeoTIFF store the opposite transform, so you'll
///   probably need to invert that before supplying it here.
#[pyfunction]
#[pyo3(
    text_signature = "(shapes, foregrounds, output_shape, background = None, dtype = None, algorithm = 'replace', geo_to_pix = None)"
)]
fn rasterize<'a>(
    py: Python<'a>,
    shapes: &PySequence,
    foregrounds: &PySequence,
    output_shape: (usize, usize),
    background: Option<&PyAny>,
    dtype: Option<&PyAny>,
    algorithm: Option<&str>,
    geo_to_pix: Option<[f64; 6]>,
) -> PyResult<&'a PyAny> {
    let shape_count: usize = shapes.len()?.try_into()?;
    if shape_count != foregrounds.len()? {
        return Err(PyValueError::new_err(format!(
            "the number of shapes ({}) must match the number of foreground elements ({})",
            shape_count,
            foregrounds.len()?
        )));
    }

    // first, convert the shapes into `geo::Geometry`
    let mut rust_shapes: Vec<Geometry<f64>> = Vec::with_capacity(shape_count);
    for i in 0..shape_count {
        let shape = shapes.get_item(i)?;
        let is_valid = shape.getattr("is_valid")?.downcast::<PyBool>()?.is_true();
        if is_valid {
            let py_wkb: &PyBytes = shape.getattr("wkb")?.downcast::<PyBytes>()?;
            let shp = wkb::wkb_to_geom(&mut Cursor::new(py_wkb.as_bytes())).unwrap();
            rust_shapes.push(shp);
        }
    }

    // then do the easy argument parsing
    let (width, height) = output_shape;
    let algorithm =
        match algorithm.unwrap_or("add") {
            "add" => MergeAlgorithm::Add,
            "replace" => MergeAlgorithm::Replace,
            _ => return Err(PyValueError::new_err(
                "Bad value for algorithm, you must supply either `'add'` or `'replace'` or `None`.",
            )),
        };
    let geo_to_pix = geo_to_pix.map(geo_rasterize::Transform::from_array);

    let np = numpy::get_array_module(py)?;

    // make both foreground and background numpy arrays
    let np_array = np.getattr("array")?;
    let foregrounds = np_array.call1((foregrounds,))?;
    let zero = py.eval("0", None, None)?;
    let background = np_array.call1((background.unwrap_or(zero),))?;

    let dtype = match dtype {
        // we can't call `np.result_type` with regular python lists,
        // it wants np arrays or dtypes, so this has to come after we
        // convert foreground and background to np.arrays.
        None => np
            .getattr("result_type")?
            .call1((foregrounds, background))?,
        Some(dtype) => np.getattr("dtype")?.call1((dtype,))?,
    };

    // now that we have dtypes and arrays, we can cast them to the
    // appropriate kind of array...
    let foregrounds = foregrounds.call_method1("astype", (dtype,))?;
    let background = background.call_method1("astype", (dtype,))?;
    let dtype: &PyArrayDescr = dtype.downcast()?;

    // This is just to bundle things that don't rely on T together to
    // reduce visual clutter when dispatching below.
    let most = MostRasterArgs {
        shapes: rust_shapes,
        width,
        height,
        algorithm,
        geo_to_pix,
    };

    Ok(match dtype.get_datatype() {
        Some(DataType::Float32) => handle_type!(f32, py, most, background, foregrounds),
        Some(DataType::Float64) => handle_type!(f64, py, most, background, foregrounds),
        Some(DataType::Uint8) => handle_type!(u8, py, most, background, foregrounds),
        Some(DataType::Uint16) => handle_type!(u16, py, most, background, foregrounds),
        Some(DataType::Uint32) => handle_type!(u32, py, most, background, foregrounds),
        Some(DataType::Uint64) => handle_type!(u64, py, most, background, foregrounds),
        Some(DataType::Int8) => handle_type!(i8, py, most, background, foregrounds),
        Some(DataType::Int16) => handle_type!(i16, py, most, background, foregrounds),
        Some(DataType::Int32) => handle_type!(i32, py, most, background, foregrounds),
        Some(DataType::Int64) => handle_type!(i64, py, most, background, foregrounds),
        _ => {
            return Err(PyTypeError::new_err(
		"Bad dtype. Acceptable values are uint8, uint16, uint32, uint64, int8, int16, int32, int64, float32, float64."));
        }
    })
}

struct MostRasterArgs {
    shapes: Vec<Geometry<f64>>,
    width: usize,
    height: usize,
    algorithm: MergeAlgorithm,
    geo_to_pix: Option<Transform>,
}

fn generic_rasterize<T>(
    most: MostRasterArgs,
    background: T,
    foregrounds: Vec<T>,
) -> Result<Array2<T>>
where
    T: Element + Copy + std::ops::Add<Output = T> + Default,
{
    let mut builder = LabelBuilder::background(background)
        .width(most.width)
        .height(most.height)
        .algorithm(most.algorithm);
    if let Some(geo_to_pix) = most.geo_to_pix {
        builder = builder.geo_to_pix(geo_to_pix);
    }
    let mut rasterizer = builder.build()?;
    for (shape, foreground) in most.shapes.into_iter().zip(foregrounds.into_iter()) {
        rasterizer.rasterize(&shape, foreground)?;
    }
    Ok(rasterizer.finish())
}

/// This is just like `generic_rasterize` but specialized for the u8
/// case where background==0 and foregrounds all equal 1, in which
/// case we can just use the `BinaryRasterizer`.
fn binary_rasterize(most: MostRasterArgs) -> Result<Array2<bool>> {
    let mut builder = BinaryBuilder::new().width(most.width).height(most.height);
    if let Some(geo_to_pix) = most.geo_to_pix {
        builder = builder.geo_to_pix(geo_to_pix);
    }
    let mut rasterizer = builder.build()?;
    for shape in most.shapes.into_iter() {
        rasterizer.rasterize(&shape)?;
    }
    Ok(rasterizer.finish()) //.mapv(|v| v as T))
}

/// A Python module implemented in Rust.
#[pymodule]
#[pyo3(name = "geo_rasterize")]
fn py_geo_rasterize(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(rasterize, m)?)?;
    Ok(())
}
