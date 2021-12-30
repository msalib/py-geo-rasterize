from shapely.geometry.base import BaseGeometry
from numpy import ndarray, dtype
from numbers import Number
from typing import Tuple, Optional, Sequence, Union

def rasterize(
        shapes: Sequence[BaseGeometry],
        foregrounds: Union[Sequence[Union[int, float]], ndarray],
        output_shape: Tuple[int, int],
        background: Optional[Union[int, float, ndarray]] = None,
        dtype: Optional[Union[str, dtype]] = None,
        algorithm: str = 'replace',
        geo_to_pix: Optional[Union[Sequence[float], ndarray]] = None) -> ndarray:
    ...
