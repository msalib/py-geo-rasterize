# geo-rasterize: a 2D rasterizer for geospatial applications, written in Rust

[![PyPI][pypi-badge]][pypi-url]
[![Build Status][actions-badge]][actions-url]

[pypi-badge]: https://img.shields.io/pypi/pyversions/geo-rasterize
[pypi-url]: https://pypi.org/project/geo-rasterize/
[actions-badge]: https://github.com/msalib/py-geo-rasterize/actions/workflows/Release.yml/badge.svg
[actions-url]: https://github.com/msalib/py-geo-rasterize/actions?query=Release+branch%3Amain

`geo-rasterize` is a Python wrapper for a [rust library with the same
name](https://crates.io/crates/geo-rasterize) that rasterizes
[shapely](https://shapely.readthedocs.io/en/stable/project.html)
vector shapes, just like
[rasterio](https://rasterio.readthedocs.io/)'s
[features.rasterize](https://rasterio.readthedocs.io/en/latest/api/rasterio.features.html#rasterio.features.rasterize). The
difference is that while [rasterio](https://rasterio.readthedocs.io/)
is built on GDAL, this library has no dependencies -- you can install
this wheel without having to worry about GDAL (or any other C library
at all)! Plus `geo-rasterize`'s rasterization algorithm is based on
GDAL's so it should be a drop in replacement for
`rasterio.features.rasterize` and it offers a very similar API.

We publish wheels to PyPI Python 3.7+ for the following platforms:

| Operating System | Architecture                    |
|------------------|---------------------------------|
| Linux            | x86-64                          |
| Linux            | i686                            |
| Linux            | aarch64                         |
| Windows          | x86-64                          |
| Windows          | i686                            |
| MacOS            | Universal2 (x86-64 and aarch64) |

## Examples

For example, let's rasterize a single `Point` located at `(x=1, y=2)`
onto a raster that is 5 pixels wide and 6 pixels high. By default, the
raster pixels will start out with value zero, and we'll put a `1` in
every pixel the point touches:

```python
>>> from shapely.geometry import Point
>>> from geo_rasterize import rasterize
>>> print(rasterize([Point(1, 2)], [1], (5, 6)))
[[0 0 0 0 0]
 [0 0 0 0 0]
 [0 1 0 0 0]
 [0 0 0 0 0]
 [0 0 0 0 0]
 [0 0 0 0 0]]

```

And the result is just what we expect: a 5x6 `numpy` array with
exactly one pixel set to 1! Note that we had to specify a list of
shapes rather than just one shape. And the list of foreground values
(`[1]` in this case) has to have the same length since we'll need one
foreground value for each shape.

So let's see multiple shapes!
```python
>>> from shapely.geometry import Point, LineString
>>> from geo_rasterize import rasterize
>>> shapes = [Point(3, 4), LineString([(0, 3), (3, 0)])]
>>> foregrounds = [3, 7]
>>> raster_size = (4, 5)
>>> print(rasterize(shapes, foregrounds, raster_size))
[[0 0 7 0]
 [0 7 7 0]
 [7 7 0 0]
 [7 0 0 0]
 [0 0 0 3]]

```

What happens when two shapes burn in the same pixel? That depends on
how you set the merge algorithm, given by the `algorithm`
parameter. The default is `'replace'` which means the last shape
overwrites the pixel but you can also set it to `'add'` to that
foreground values will sum. That allows you to make heatmaps!

```python
>>> from shapely.geometry import Point, LineString
>>> from geo_rasterize import rasterize
>>> shapes = [LineString([(0, 0), (5, 5)]), LineString([(5, 0), (0, 5)])]
>>> print(rasterize(shapes, [1, 1], (5, 5), algorithm='add'))
[[1 0 0 0 1]
 [0 1 0 1 1]
 [0 0 2 1 0]
 [0 1 1 1 0]
 [1 1 0 0 1]]

```

Our two lines cross at the center where you'll find `2`.

You can change the default value using the `background` parameter. And
you can set the array `dtype` using the `dtype` parameter. Setting
`dtype='uint8'` will reduce the space needed for your raster array by
8x. This is especially useful if you're only interested in binary
rasterization.

## Geographic transforms

All our examples so far have assumed that our shapes' coordinates are
in the image space. In other words, we've assumed that the `x`
coordinates will be in the range `0..width` and the `y` coordinates
will be in the range `0..height`. Alas, that is often not the case!

For satellite imagery (or remote sensing imagery in general), images
will almost always specify both a Coordinate Reference System
([CRS](https://en.wikipedia.org/wiki/Spatial_reference_system)) and an
affine transformation in their metadata. See [rasterio's
Georeferencing](https://rasterio.readthedocs.io/en/latest/topics/georeferencing.html)
for more details.

In order to work with most imagery, you have to convert your vector
shapes from whatever their original CRS is (often `EPSG:4326` for
geographic longitude and latitude) into whatever CRS your data file
specifies (often a
[UTM](https://en.wikipedia.org/wiki/Universal_Transverse_Mercator_coordinate_system)
projection but there are so many choices). Then, you need to apply an
affine transformation to convert from world coordinates to pixel
coordinates. Since raster imagery usually specifies the inverse
transformation matrix (i.e. a `pix_to_geo` transform), you'll first
need to invert it to get a `geo_to_pix` transform before applying it
to the coordinates. And now you've got pixel coordinates appropriate
for your image data!

`geo-raterize` can ease this tedious process by taking care of the
affine transformation. Just pass an affine transform array using the
`geo_to_pix` parameter (call `.to_gdal()` if you have an
`affine.Affine` instance).

To summarize, you'll have to:

* extract the CRS from your image and convert your shapes into that
  CRS (probably using [pyproj](https://pyproj4.github.io/pyproj/stable/)
  and its integration with [geo types][geo],
* extract the `pix_to_geo` transform from your imagery metadata
* create an `Affine` instance from that data (GDAL represents these
  as a `[f64; 6]` array and you can use `Affine.from_gdal`)
* call `transform.inverse` to get the corresponding `geo_to_pix`
  transform (remember that not all transforms are invertible!)
* call `transform.to_gdal()` and use the resulting array with the
  `geo_to_pix` parameter
