import pytest
import os
import subprocess
from PIL import Image

EXECUTABLE_PATH = "../target/release/sparrow"
TEST_IMGS_DIR_NAME = "png_testsuite"

def test_executable_exists():
    """
        Test that ../target/release/sparrow exists
    """
    fail = False
    try:
        with open(EXECUTABLE_PATH) as f:
            pass
    except IOError:
        fail = True
    assert not fail, "Executable not available"

def get_image_names():
    result = [TEST_IMGS_DIR_NAME + "/" + f for f in os.listdir(TEST_IMGS_DIR_NAME) if "png" in f]
    result.sort()

    return result

@pytest.mark.parametrize("name", get_image_names())
def test_load_image(name):
    """
        Compare output from Sparrow to Pillow
    """
    subprocess.run(["rm", "img.ppm"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    should_fail = False
    try:
        img = Image.open(name)
        img = img.convert(mode="RGBA")
        original = img.getdata()
    except:
        should_fail = True

    should_fail = should_fail or name[len(TEST_IMGS_DIR_NAME + "/")] == 'x'

    result = subprocess.run([EXECUTABLE_PATH, name], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    assert (result.returncode != 0) == should_fail, "Image loading failed: {}".format(result.stderr)
    if should_fail:
        return

    pixels = []
    with open("img.ppm") as f:
        assert f.readline().strip() == "P3"
        w, h = f.readline().strip().split(" ")
        max_val = f.readline().strip()
        for i in range(int(w) * int(h)):
            data = f.readline().strip().split(" ")
            pixels.append(( int(data[0]), int(data[1]), int(data[2]), int(data[3]) ))

    for x, y in zip(pixels, original):
        assert x == y
