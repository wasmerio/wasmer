#include <iostream>

void bar() { throw 40; }

void foo() { bar(); }

void baz() {
  try {
    foo();
  } catch (int &e) {
    e += 2;
    std::cout << "caught exception, will rethrow" << std::endl;
    throw;
  }
}

int main() {
  try {
    baz();
  } catch (int myNum) {
    std::cout << "caught exception in main: " << myNum << std::endl;
    return myNum;
  }

  return 1;
}
