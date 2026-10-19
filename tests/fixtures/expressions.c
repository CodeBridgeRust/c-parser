int expr_test(void) {
    int a = 1 + 2 * 3;
    int b = (1 + 2) * 3;
    int c = a > b ? a : b;
    int d = sizeof(int);
    int e = a && b || !c;
    a++;
    --b;
    return a + b - c * d / e;
}