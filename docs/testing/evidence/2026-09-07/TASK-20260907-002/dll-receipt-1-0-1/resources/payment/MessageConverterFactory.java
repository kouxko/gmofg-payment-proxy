/*-------------------------------------------------------------------------------------------------
 -                                                                                                -
 - Copyright (c)                                                                                  -
 - PAX Computer Technology(Shenzhen) CO., LTD PROPRIETARY INFORMATION                             -
 - This software is supplied under the terms of a license agreement or nondisclosure              -
 - agreement with PAX Computer Technology(Shenzhen) CO., LTD and may not be copied or             -
 - disclosed except in accordance with the terms in that agreement.                               -
 - Copyright (c) 2020. PAX Computer Technology(Shenzhen) CO., LTD All rights reserved.            -
 -                                                                                                -
 -                                                                                                -
 -                                                                                                -
 -------------------------------------------------------------------------------------------------*/
package jp.gmofg.payment.https.client.converters;

import android.text.TextUtils;
import java.io.IOException;
import java.lang.annotation.Annotation;
import java.lang.reflect.Type;
import java.text.SimpleDateFormat;
import java.util.Locale;
import jp.gmofg.common.util.PLog;
import jp.gmofg.common.util.StringUtil;
import jp.gmofg.core.NeptuneLiteProvider;
import jp.gmofg.msg.Message;
import jp.gmofg.msg.dll.ConnectionTest;
import jp.gmofg.msg.dll.CreditDLL;
import jp.gmofg.msg.dll.UnionPayDLL;
import jp.gmofg.msg.receipt.JsonReceipt;
import jp.gmofg.msg.trans.credit.Credit;
import jp.gmofg.msg.trans.unionpay.UnionPay;
import jp.gmofg.payment.settings.AppPreference;
import jp.gmofg.payment.settings.LauncherSettings;
import okhttp3.RequestBody;
import okhttp3.ResponseBody;
import retrofit2.Converter;
import retrofit2.Retrofit;

/**
 * message converter factory for retrofit;
 */
public class MessageConverterFactory extends Converter.Factory {

    /**
     * create instance
     */
    private MessageConverterFactory() {
    }

    /**
     * factory method
     *
     * @return message converter factory
     */
    public static MessageConverterFactory create() {
        return new MessageConverterFactory();
    }

    private static <T extends Message.Response> T timeCorrection(T response) {
        if (!LauncherSettings.isTrainingMode() && response != null) {
            try {
                final String dateTimeStr =
                        new SimpleDateFormat("yyMMddHHmmss", Locale.JAPAN).format(
                                response.getResponseTimeStamp());
                final String currentTIme = NeptuneLiteProvider.getSys().getDate();
                PLog.d(String.format("current time : %s response time: %s", currentTIme,
                        dateTimeStr));
                if (TextUtils.equals(currentTIme, dateTimeStr)) {
                    PLog.d("updateToLauncher time: " + dateTimeStr);
                    NeptuneLiteProvider.getSys().setDate(dateTimeStr);
                }
            } catch (Throwable t) {
                PLog.d("skip time correction!");
            }
        }
        return response;
    }

    /**
     * Request body converter converter.
     *
     * @param type the type
     * @param parameterAnnotations the parameter annotations
     * @param methodAnnotations the method annotations
     * @param retrofit the retrofit
     * @return the converter
     * @see Converter.Factory
     */
    @Override
    public Converter<?, RequestBody> requestBodyConverter(Type type,
            Annotation[] parameterAnnotations, Annotation[] methodAnnotations, Retrofit retrofit) {
        if (type instanceof Class<?> && JsonReceipt.Request.class.isAssignableFrom(
                (Class<?>) type)) {
            return JsonReceiptRequestBodyConverter.INSTANCE;
        }
        if (type instanceof Class<?> && Message.Request.class.isAssignableFrom((Class<?>) type)) {
            return MessageRequestBodyConverter.INSTANCE;
        }
        return super.requestBodyConverter(type, parameterAnnotations, methodAnnotations, retrofit);
    }

    /**
     * Response body converter converter.
     *
     * @param type the type
     * @param annotations the annotations
     * @param retrofit the retrofit
     * @return the converter
     * @see Converter.Factory
     */
    @Override
    public Converter<ResponseBody, ?> responseBodyConverter(Type type, Annotation[] annotations,
            Retrofit retrofit) {

        if (type == Credit.Response.class) {
            return CreditMessageResponseBodyConverter.INSTANCE;
        }

        if (type == UnionPay.Response.class) {
            return UnionPayMessageResponseBodyConverter.INSTANCE;
        }

        if (type == CreditDLL.Response.class) {
            return CreditDLLMessageResponseBodyConverter.INSTANCE;
        }

        if (type == UnionPayDLL.Response.class) {
            return UnionPayDLLMessageResponseBodyConverter.INSTANCE;
        }

        if (type == ConnectionTest.Response.class) {
            return ConnectionTestResponseBodyConverter.INSTANCE;
        }

        if (type == JsonReceipt.Response.class) {
            return JsonReceiptResponseBodyConverter.INSTANCE;
        }

        return super.responseBodyConverter(type, annotations, retrofit);
    }

    /**
     * request message converter
     */
    static class MessageRequestBodyConverter implements Converter<Message.Request, RequestBody> {

        /**
         * The Instance.
         */
        static MessageRequestBodyConverter INSTANCE = new MessageRequestBodyConverter();

        /**
         * Convert request body.
         *
         * @param value the value
         * @return the request body
         */
        @Override
        public RequestBody convert(Message.Request value) {
            String requestBody = value.toJsonString();
            PLog.d(requestBody);
            return RequestBody.create(null, StringUtil.decodeShiftJIS(requestBody));
        }
    }

    /**
     * request message converter
     */
    static class JsonReceiptRequestBodyConverter
            implements Converter<JsonReceipt.Request, RequestBody> {

        /**
         * The Instance.
         */
        static JsonReceiptRequestBodyConverter INSTANCE = new JsonReceiptRequestBodyConverter();

        /**
         * Convert request body.
         *
         * @param value the value
         * @return the request body
         */
        @Override
        public RequestBody convert(JsonReceipt.Request value) {
            String requestBody = value.toJsonString();
            PLog.d(requestBody);
            return RequestBody.create(null, StringUtil.decodeShiftJIS(requestBody));
        }
    }

    /**
     * response message converter
     */
    static class CreditDLLMessageResponseBodyConverter
            implements Converter<ResponseBody, CreditDLL.Response> {

        /**
         * The Instance.
         */
        static CreditDLLMessageResponseBodyConverter INSTANCE =
                new CreditDLLMessageResponseBodyConverter();

        /**
         * Convert credit dll . response.
         *
         * @param value the value
         * @return the credit dll . response
         * @throws IOException the io exception
         */
        @Override
        public CreditDLL.Response convert(ResponseBody value) throws IOException {
            try {
                String responseBody = StringUtil.encodeShiftJIS(value.bytes());
                PLog.d(responseBody);
                return CreditDLL.responseFrom(responseBody);
            } catch (Throwable e) {
                throw new MessageConvertException();
            }
        }
    }

    /**
     * response message converter
     */
    static class UnionPayDLLMessageResponseBodyConverter
            implements Converter<ResponseBody, UnionPayDLL.Response> {

        /**
         * The Instance.
         */
        static UnionPayDLLMessageResponseBodyConverter INSTANCE =
                new UnionPayDLLMessageResponseBodyConverter();

        /**
         * Convert union pay dll . response.
         *
         * @param value the value
         * @return the union pay dll . response
         * @throws IOException the io exception
         */
        @Override
        public UnionPayDLL.Response convert(ResponseBody value) throws IOException {
            try {
                String responseBody = StringUtil.encodeShiftJIS(value.bytes());
                PLog.d(responseBody);
                return UnionPayDLL.responseFrom(responseBody);
            } catch (Throwable e) {
                throw new MessageConvertException();
            }
        }
    }

    /**
     * response message converter
     */
    static class CreditMessageResponseBodyConverter
            implements Converter<ResponseBody, Credit.Response> {

        /**
         * The Instance.
         */
        static CreditMessageResponseBodyConverter INSTANCE =
                new CreditMessageResponseBodyConverter();

        /**
         * Convert credit . response.
         *
         * @param value the value
         * @return the credit . response
         * @throws IOException the io exception
         */
        @Override
        public Credit.Response convert(ResponseBody value) throws IOException {
            try {
                String responseBody = StringUtil.encodeShiftJIS(value.bytes());
                PLog.d(responseBody);
                return timeCorrection(Credit.responseFrom(responseBody));
            } catch (Throwable e) {
                throw new MessageConvertException();
            }
        }
    }

    /**
     * response message converter
     */
    static class UnionPayMessageResponseBodyConverter
            implements Converter<ResponseBody, UnionPay.Response> {

        /**
         * The Instance.
         */
        static UnionPayMessageResponseBodyConverter INSTANCE =
                new UnionPayMessageResponseBodyConverter();

        /**
         * Convert union pay . response.
         *
         * @param value the value
         * @return the union pay . response
         * @throws IOException the io exception
         */
        @Override
        public UnionPay.Response convert(ResponseBody value) throws IOException {
            try {
                String responseBody = StringUtil.encodeShiftJIS(value.bytes());
                PLog.d(responseBody);
                return timeCorrection(UnionPay.responseFrom(responseBody));
            } catch (Throwable e) {
                throw new MessageConvertException();
            }
        }
    }

    /**
     * response message converter
     */
    static class ConnectionTestResponseBodyConverter
            implements Converter<ResponseBody, ConnectionTest.Response> {

        /**
         * The Instance.
         */
        static ConnectionTestResponseBodyConverter INSTANCE =
                new ConnectionTestResponseBodyConverter();

        /**
         * Convert union pay . response.
         *
         * @param value the value
         * @return the union pay . response
         * @throws IOException the io exception
         */
        @Override
        public ConnectionTest.Response convert(ResponseBody value) throws IOException {
            try {
                String responseBody = StringUtil.encodeShiftJIS(value.bytes());
                PLog.d(responseBody);
                return timeCorrection(ConnectionTest.responseFrom(responseBody));
            } catch (Throwable e) {
                throw new MessageConvertException();
            }
        }
    }

    /**
     * response message converter
     */
    static class JsonReceiptResponseBodyConverter
            implements Converter<ResponseBody, JsonReceipt.Response> {

        /**
         * The Instance.
         */
        static JsonReceiptResponseBodyConverter INSTANCE = new JsonReceiptResponseBodyConverter();

        /**
         * Convert union pay . response.
         *
         * @param value the value
         * @return the union pay . response
         * @throws IOException the io exception
         */
        @Override
        public JsonReceipt.Response convert(ResponseBody value) throws IOException {
            try {
                String responseBody = StringUtil.encodeShiftJIS(value.bytes());
                PLog.d(responseBody);
                return JsonReceipt.responseFrom(responseBody);
            } catch (Throwable e) {
                throw new MessageConvertException();
            }
        }
    }

    /**
     * The type Message convert exception.
     */
    public static class MessageConvertException extends IOException {

    }
}

