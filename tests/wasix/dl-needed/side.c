extern int side_needed_func(int);

int side_func(int x)
{
    return side_needed_func(x) * 2;
}