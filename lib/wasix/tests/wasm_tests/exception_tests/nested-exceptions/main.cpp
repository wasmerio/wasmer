// Originally reported: https://github.com/wasmerio/wasmer/issues/6271

#include <iostream>

int main() {
  try {
    try {
      throw 1;
    } catch (int e) {
      std::cout << "inner " << e << std::endl;
      throw 2;
    }
  } catch (int e) {
    std::cout << "outer " << e << std::endl;
    return 0;
  }

  return 1;
}
