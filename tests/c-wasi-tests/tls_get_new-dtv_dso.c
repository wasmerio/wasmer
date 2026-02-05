__thread char v[123];
__thread int x = 42;
__thread long double y;

void *f(void)
{
    for (int i = 0; i < (int)sizeof v; i++)
        v[i] = (char)(i % 16);
    return v;
}

void *g(void)
{
    return &x;
}

void *h(void)
{
    return &y;
}
