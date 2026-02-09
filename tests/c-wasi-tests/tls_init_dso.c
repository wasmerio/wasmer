static char buf[] = "foobar";
__thread char *tls = buf;

char *gettls(void)
{
    return tls;
}
