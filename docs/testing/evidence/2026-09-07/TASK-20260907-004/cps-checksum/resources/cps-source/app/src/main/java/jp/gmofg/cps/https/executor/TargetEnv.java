package jp.gmofg.cps.https.executor;

/**
 * 生产环境或测试环境的后台配置
 */
public enum TargetEnv {

    Stub("gateway/stub-products-pointPay", "gateway/stub-products-pointConfirm", "gateway/stub-products-refund", "gateway/stub-products-reverse"),

    Product("gateway/products-pointPay", "gateway/products-pointConfirm", "gateway/products-refund", "gateway/products-reverse"),

    Old_Stub("gateway/stubPay", "gateway/stubConfirm", "gateway/stubRefund", "gateway/stubReverse"),

    Old_Product("gateway/pay", "gateway/confirm", "gateway/refund", "gateway/reverse");

    static final String ENV_PRODUCT_KEY = "1";

    static final String ENV_OLD_STUB_KEY = "2";

    static final String ENV_OLD_PRODUCT_KEY = "3";

    final String payPath;
    public final String confirmPath;
    final String refundPath;
    final String reversePath;

    TargetEnv(final String payPath, final String confirmPath, final String refundPath, final String reversePath) {
        this.payPath = payPath;
        this.confirmPath = confirmPath;
        this.refundPath = refundPath;
        this.reversePath = reversePath;
    }

}