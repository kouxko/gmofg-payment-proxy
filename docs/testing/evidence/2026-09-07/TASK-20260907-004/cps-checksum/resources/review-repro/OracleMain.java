import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import java.io.IOException;
import java.lang.reflect.Field;
import java.nio.charset.Charset;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.Comparator;
import java.util.List;

public final class OracleMain {
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();

    private OracleMain() {}

    public static final class TextUtils {
        static boolean isEmpty(CharSequence value) {
            return value == null || value.length() == 0;
        }
    }

    public static final class Logger {
        static void error(String ignored) {}
    }

    public static class Meta {
        private String code;
        private String message;
    }

    public static class BaseResult {
        private String orderDetailId;
        private String orderId;
        private String transTime;
        private String transStatus;
    }

    public static class BaseData {
        private String errorCode;
        private String errorInfo;
        private String subErrorCode;
        private String sign;
    }

    public static final class PayResult extends BaseResult {
        private String currencyCode;
        private int amount;
        private int amountRmb;
        private String payType;
        private String extendedInfo;
    }

    public static final class ConfirmResult extends BaseResult {
        private String payType;
        private String payCheckDate;
    }

    public static final class RefundResult extends BaseResult {
        private String currencyCode;
        private int refundAmount;
    }

    public static final class ReverseResult extends BaseResult {
        private int amount;
        private String currencyCode;
    }

    public static final class PayData extends BaseData { private PayResult result; }
    public static final class ConfirmData extends BaseData { private ConfirmResult result; }
    public static final class RefundData extends BaseData { private RefundResult result; }
    public static final class ReverseData extends BaseData { private ReverseResult result; }

    public static final class PayResponse extends Message.Response { private PayData data; }
    public static final class ConfirmResponse extends Message.Response { private ConfirmData data; }
    public static final class RefundResponse extends Message.Response { private RefundData data; }
    public static final class ReverseResponse extends Message.Response { private ReverseData data; }

    // These types are part of the original algorithm's explicit instanceof allow-list.
    // They are unused by the downstream response vectors but must exist for exact compilation.
    public static class Products {}
    public static class VoucherDiscount {}
    public static class ProductsDetail {}

    public static final class Message {
        private static String lastCanonical;

        public static class Request {}
        public static class Response { private Meta meta; }

    /**
     * calc sign for online request or response
     * @param requestOrResponse
     * @param token
     * @return
     */
    public static String calcSignWithToken(Object requestOrResponse, String token){
        boolean isRequestOrResponse
                = requestOrResponse instanceof Message.Request
                || requestOrResponse instanceof Message.Response;
        if (!isRequestOrResponse){
            return "";
        }
        List<Field> fields = getFieldList(requestOrResponse);
        Collections.sort(fields, new FieldComparator());//内存泄漏！！！
        StringBuilder stringBuilder = new StringBuilder();
        stringBuilder.append("{");
        stringBuilder.append(getSortedNameValuePair(requestOrResponse));
        stringBuilder.append("&key=").append("B2B").append(token);
        stringBuilder.append("}");
        lastCanonical = stringBuilder.toString(); return md5(lastCanonical).toUpperCase();
    }


    /**
     *
     * @param object
     * @return
     */
    public static String getSortedNameValuePair(Object object){
        if (object instanceof Message.Request
                || object instanceof Message.Response
                || object instanceof Meta
                || object instanceof BaseData
                || object instanceof BaseResult
                || object instanceof Products
                || object instanceof VoucherDiscount
                || object instanceof ProductsDetail) {
            List<Field> fields = getFieldList(object);
            Collections.sort(fields, new FieldComparator());//内存泄漏！！！
            StringBuilder stringBuilder = new StringBuilder();
            for (Field field : fields){
                if (!isNotIncludedInSign(object, field)) {
                    try {
                        Object valueObj = field.get(object);
                        stringBuilder.append(field.getName()).append("=").append(getSortedNameValuePair(valueObj).replaceAll(" ", "")).append("&");
                    } catch (Exception e){
                        Logger.error(e.getMessage());
                    }
                }
            }
            String nameValuePairs = stringBuilder.toString();
            if (stringBuilder.length() > 0){
                nameValuePairs = stringBuilder.substring(0, stringBuilder.length() - 1);
            }
            if (object instanceof Meta
                    || object instanceof BaseData
                    || object instanceof BaseResult
                    || object instanceof Products
                    || object instanceof VoucherDiscount
                    || object instanceof ProductsDetail){
                return "{" + nameValuePairs + "}";
            }
            return nameValuePairs;
        }
        return object.toString();
    }

    /**
     * get all filed for calculating sign
     * @param obj
     * @return
     */
    private static List<Field> getFieldList(Object obj){
        Class<?> cls = obj.getClass();
        List<Field> fieldList = new ArrayList<>();
        while (null != cls){
            fieldList.addAll(Arrays.asList(cls.getDeclaredFields()));
            cls = cls.getSuperclass();
        }
        return fieldList;
    }


    /**
     *
     * @param obj
     * @param field
     * @return true if this filed should not included in sign
     */
    private static boolean isNotIncludedInSign(Object obj, Field field){

        String name = field.getName();
        if (isUseless(name)){
            return true;
        }

        try {
            field.setAccessible(true);
            Object value = field.get(obj);
            return isEmpty(value);
        } catch (Exception e){
            return true;
        }
    }

    private static boolean isUseless(String filedName){
        //shadow$_klass_ and shadow$_monitor_ and sign(in Response)
        if (!TextUtils.isEmpty(filedName) &&
                (filedName.contains("shadow") || filedName.equals("sign"))){
            return true;
        }
        return false;
    }

    /**
     * is this filed is empty
     * default value is empty too
     * @param value
     * @return
     */
    private static boolean isEmpty(Object value){
        if (null == value){
            return true;
        }
        if (value instanceof Integer && ((Integer) value).intValue() == 0){
            return true;
        }
        return false;
    }

    /**
     * sort the filed by name, before calculating sign
     */
    private static class FieldComparator implements Comparator<Field> {
        @Override
        public int compare(Field field1, Field field2) {
            return field1.getName().compareTo(field2.getName());
        }
    }


    /**
     * md 5 string.
     *
     * @param str the str
     * @return the string
     */
    private static String md5(String str) {
        try {
            MessageDigest digest = MessageDigest.getInstance("MD5");
            digest.update(str.getBytes());
            byte[] messageDigest = digest.digest();
            // Create Hex String
            StringBuilder hexString = new StringBuilder();

            for (byte i : messageDigest) {
                String shaHex = Integer.toHexString(i & 0xFF);
                if (shaHex.length() < 2) {
                    hexString.append(0);
                }
                hexString.append(shaHex);
            }
            return hexString.toString();

        } catch (NoSuchAlgorithmException e) {
            //throw runtime exception
            throw new RuntimeException(e);
        }
    }
    }

    private record Case(String id, String operation, String body, String token, boolean expectError) {}

    private static List<Case> cases() {
        String paddedToken = "MID-01      TERM-01      20260907123456000000000000100";
        return List.of(
            new Case("edge-exp-lower", "confirm", "{\"meta\":{\"code\":1e2},\"data\":{\"sign\":\"OLD\",\"result\":{}}}", "TOKEN", false),
            new Case("edge-exp-upper", "confirm", "{\"meta\":{\"code\":1E+2},\"data\":{\"sign\":\"OLD\",\"result\":{}}}", "TOKEN", false),
            new Case("edge-negative-zero", "confirm", "{\"meta\":{\"code\":-0},\"data\":{\"sign\":\"OLD\",\"result\":{}}}", "TOKEN", false),
            new Case("pay-before", "pay", """
                {"meta":{"code":"00","message":"OK"},"data":{"errorCode":"","errorInfo":null,"subErrorCode":"","sign":"OLD","result":{"currencyCode":"JPY","amount":100,"amountRmb":0,"payType":"99","extendedInfo":"paired","orderDetailId":"PAIR-D","orderId":"PAIR-O","transTime":"20260907123456","transStatus":"PAY_SUCCESS"}}}
                """.trim(), "MERCHANT  TERMINAL  TOKEN", false),
            new Case("pay-after", "pay", """
                {"meta":{"code":"00","message":"OK"},"data":{"errorCode":"","errorInfo":null,"subErrorCode":"","sign":"OLD","result":{"currencyCode":"JPY","amount":250,"amountRmb":0,"payType":"99","extendedInfo":"paired","orderDetailId":"PAIR-D","orderId":"PAIR-O","transTime":"20260907123456","transStatus":"PAY_SUCCESS"}}}
                """.trim(), "MERCHANT  TERMINAL  TOKEN", false),
            new Case("pay-normal-inherited-zero", "pay", """
                {"meta":{"code":"00","message":"OK"},"data":{"errorCode":"","errorInfo":null,"subErrorCode":"S 1","sign":"IGNORED","result":{"currencyCode":"JPY","amount":100,"amountRmb":0,"payType":"99","extendedInfo":"A B","orderDetailId":"D1","orderId":"O1","transTime":"2026-09-07 12:34:56","transStatus":"PAY_SUCCESS"}},"unknownTop":"ignored"}
                """.trim(), paddedToken, false),
            new Case("pay-business-error", "pay", """
                {"meta":{"code":"09","message":"business error"},"data":{"errorCode":"09103","errorInfo":"not found","subErrorCode":"","sign":"OLD","result":null}}
                """.trim(), paddedToken, false),
            new Case("confirm-normal-unicode-whitespace-unknown", "confirm", """
                {"meta":{"code":"00","message":" 日 本\\t行\\n次 "},"data":{"sign":"OLD","result":{"payType":"A B","payCheckDate":"確認 日","orderDetailId":"D 2","orderId":"O2","transTime":"T\\t2","transStatus":"SUCCESS","unknownNested":123}},"unknownData":{"x":1}}
                """.trim(), paddedToken, false),
            new Case("confirm-business-error-string-coercion", "confirm", """
                {"meta":{"code":9,"message":true},"data":{"errorCode":931,"errorInfo":false,"subErrorCode":301,"sign":"OLD","result":{"payType":99,"payCheckDate":20260907,"transStatus":false}}}
                """.trim(), paddedToken, false),
            new Case("refund-normal-int-string", "refund", """
                {"meta":{"code":"00","message":"OK"},"data":{"sign":"OLD","result":{"currencyCode":"JPY","refundAmount":"500","orderDetailId":"RD1","orderId":"RO1","transTime":"RT1","transStatus":"REFUND_SUCCESS"}}}
                """.trim(), paddedToken, false),
            new Case("refund-business-error-int-decimal", "refund", """
                {"meta":{"code":"09","message":"NG"},"data":{"errorCode":"09301","errorInfo":"failed","subErrorCode":"E1","sign":"OLD","result":{"currencyCode":"JPY","refundAmount":1.0,"transStatus":"REFUND_FAILED"}}}
                """.trim(), paddedToken, false),
            new Case("reverse-normal-empty-and-zero", "reverse", """
                {"meta":{"code":"00","message":""},"data":{"errorCode":null,"errorInfo":"","subErrorCode":null,"sign":"OLD","result":{"amount":0,"currencyCode":"","orderDetailId":"D3","orderId":"O3","transTime":"T3","transStatus":"REVERSE_SUCCESS"}}}
                """.trim(), paddedToken, false),
            new Case("reverse-business-error", "reverse", """
                {"meta":{"code":"09","message":"not found"},"data":{"errorCode":"09103","errorInfo":"none","subErrorCode":"","sign":"OLD","result":{"amount":25,"currencyCode":"JPY","transStatus":"REVERSE_FAILED"}}}
                """.trim(), paddedToken, false),
            new Case("pay-invalid-int-string", "pay", """
                {"meta":{"code":"00"},"data":{"sign":"OLD","result":{"amount":"abc"}}}
                """.trim(), paddedToken, true),
            new Case("refund-invalid-int-object", "refund", """
                {"meta":{"code":"00"},"data":{"sign":"OLD","result":{"refundAmount":{}}}}
                """.trim(), paddedToken, true),
            new Case("reverse-invalid-fractional-int", "reverse", """
                {"meta":{"code":"00"},"data":{"sign":"OLD","result":{"amount":1.5}}}
                """.trim(), paddedToken, true),
            new Case("confirm-invalid-string-object", "confirm", """
                {"meta":{"code":{}},"data":{"sign":"OLD","result":{}}}
                """.trim(), paddedToken, true)
        );
    }

    private static Message.Response parse(Case testCase) {
        return switch (testCase.operation()) {
            case "pay" -> GSON.fromJson(testCase.body(), PayResponse.class);
            case "confirm" -> GSON.fromJson(testCase.body(), ConfirmResponse.class);
            case "refund" -> GSON.fromJson(testCase.body(), RefundResponse.class);
            case "reverse" -> GSON.fromJson(testCase.body(), ReverseResponse.class);
            default -> throw new IllegalArgumentException("unknown operation: " + testCase.operation());
        };
    }

    private static JsonElement generate() {
        var output = new com.google.gson.JsonArray();
        for (Case testCase : cases()) {
            JsonObject row = new JsonObject();
            row.addProperty("id", testCase.id());
            row.addProperty("operation", testCase.operation());
            row.addProperty("body", testCase.body());
            row.addProperty("token", testCase.token());
            try {
                Message.Response response = parse(testCase);
                String sign = Message.calcSignWithToken(response, testCase.token());
                if (testCase.expectError()) {
                    throw new AssertionError(testCase.id() + " unexpectedly parsed");
                }
                row.addProperty("canonical", Message.lastCanonical);
                row.addProperty("sign", sign);
            } catch (RuntimeException error) {
                if (!testCase.expectError()) {
                    throw error;
                }
                row.addProperty("expect_error", true);
                row.addProperty("error_kind", rootCause(error).getClass().getSimpleName());
            }
            output.add(row);
        }
        return output;
    }

    private static Throwable rootCause(Throwable error) {
        Throwable current = error;
        while (current.getCause() != null) current = current.getCause();
        return current;
    }

    public static void main(String[] args) throws IOException {
        if (!StandardCharsets.UTF_8.equals(Charset.defaultCharset())) {
            throw new AssertionError("default charset must be UTF-8, got " + Charset.defaultCharset());
        }
        JsonElement actual = generate();
        if (args.length == 0 || "--generate".equals(args[0])) {
            System.out.println(GSON.toJson(actual));
            return;
        }
        if (args.length == 2 && "--verify".equals(args[0])) {
            JsonElement expected = JsonParser.parseString(Files.readString(Path.of(args[1]), StandardCharsets.UTF_8));
            if (!expected.equals(actual)) {
                System.err.println("expected:\n" + GSON.toJson(expected));
                System.err.println("actual:\n" + GSON.toJson(actual));
                throw new AssertionError("fixture mismatch");
            }
            System.out.println("PASS: " + actual.getAsJsonArray().size() + " Java CPS signature vectors verified with " + Charset.defaultCharset());
            return;
        }
        throw new IllegalArgumentException("usage: OracleMain [--generate | --verify <fixture>]");
    }
}
