int max(int a, int b) {
    if (a > b) {
        return a;
    } else {
        return b;
    }
}

int factorial(int n) {
    int result = 1;
    while (n > 1) {
        result = result * n;
        n = n - 1;
    }
    return result;
}

int sum_to(int n) {
    int s = 0;
    for (int i = 0; i <= n; i++) {
        s = s + i;
    }
    return s;
}