import pytest
import os
import subprocess
from PIL import Image

EXECUTABLE_PATH = "../target/release/sparrow"

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
    dir_name = "png_testsuite"
    result = [dir_name + "/" + f for f in os.listdir(dir_name) if "png" in f]
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
    except:
        should_fail = True

    result = subprocess.run([EXECUTABLE_PATH, name], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode is not 0:
        print(result.stderr)
    assert (result.returncode != 0) == should_fail
    if should_fail:
        return

    original = img.getdata()
    length = 1 if isinstance(original[0], int) else 3
    pixels = []
    with open("img.ppm") as f:
        assert f.readline().strip() == "P3"
        w, h = f.readline().strip().split(" ")
        max_val = f.readline().strip()
        for i in range(int(w) * int(h)):
            data = f.readline().strip().split(" ")
            if length == 1:
                pixels.append(int(data[0]))
            elif length == 3:
                pixels.append((int(data[0]), int(data[1]), int(data[2])))
            else:
                assert False

    for x, y in zip(pixels, original):
        assert x == y
