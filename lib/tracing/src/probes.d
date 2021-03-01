provider wasmer {
         probe instance__start();
         probe instance__end();

         probe function__start();
         probe function__invoke2(int, int);
         probe function__end();
};