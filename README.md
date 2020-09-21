# Sparrow

An image decoder library written in pure Rust.

## Currently supported

* PNG:
  * 1,2,4 and 8-bit images.
  * All chunks defined by the specification.

## Planned

* PNG: 16-bit images
* JPEG: TBD what formats
* GIF: TBD what formats

## Tests

The folder "tests" contain tests that should be run against every commit to make sure that nothing breaks. Currently png_testsuite is included and checked. The tests load the reference image using Pillow(python) and Sparrow and do a pixel by pixel comparison.

To run the tests:
```
cd tests
python3 -m venv venv
source venv/bin/activate
pip install -r requirements.txt

pytest -s -v
```
