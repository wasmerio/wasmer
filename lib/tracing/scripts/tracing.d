#!/usr/sbin/dtrace -s

#pragma D option quiet

BEGIN
{
        printf("From DTrace (wasmer-tracing)\n");
}

wasmer*:::instance-start
{
        printf("instance starts\n");
}

wasmer*:::instance-end
{
        printf("instance ends\n");
}

wasmer*:::function-start
{
        printf("function starts\n");
}

wasmer*:::function-invoke2
{
        printf("function invoked with arg0=%d, arg1=%d\n", arg0, arg1);
}

wasmer*:::function-end
{
        printf("function ends\n");
}