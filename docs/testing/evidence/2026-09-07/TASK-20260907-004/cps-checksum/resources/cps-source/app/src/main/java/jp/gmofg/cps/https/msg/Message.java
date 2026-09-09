/*
 * ===========================================================================================
 * = COPYRIGHT
 *          PAX Computer Technology(Shenzhen) CO., LTD PROPRIETARY INFORMATION
 *   This software is supplied under the terms of a license agreement or nondisclosure
 *   agreement with PAX Computer Technology(Shenzhen) CO., LTD and may not be copied or
 *   disclosed except in accordance with the terms in that agreement.
 *     Copyright (C) 2022-? PAX Computer Technology(Shenzhen) CO., LTD All rights reserved.
 * Revision History:
 * Date	                Author	                Action
 * 20220321  	        JerryJia               	Create
 * ===========================================================================================
 */
package jp.gmofg.cps.https.msg;

import android.text.TextUtils;

import java.lang.reflect.Field;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.Comparator;
import java.util.List;

import jp.gmofg.cps.R;
import jp.gmofg.cps.app.MyApp;
import jp.gmofg.cps.https.Result;
import jp.gmofg.cps.https.Utils;
import jp.gmofg.cps.https.constants.ResponseConstants;
import jp.gmofg.cps.https.msg.products.Products;
import jp.gmofg.cps.https.msg.products.ProductsDetail;
import jp.gmofg.cps.https.msg.products.VoucherDiscount;
import jp.gmofg.cps.utils.ErrorCodeInfoUtils;
import jp.gmofg.cps.utils.SubErrCodeInfoUtils;
import jp.gmofg.cps.utils.TransStatusErrorInfoUtils;
import jp.gmofg.log.Logger;


public class Message {

    // the base request for online request
    public static class Request{
        private String locale = "JAPAN";//optional
        private String timeZone = "p9";
        private String branchCode;
        private String terminalCode;//optional
        protected String sign;

        protected Request(){
            branchCode = Utils.getSpString(R.string.branch_code);
            terminalCode = Utils.getSpString(R.string.terminal_id);
        }

        protected void generateSign(String token){
            sign = calcSignWithToken(this, token);
        }

        protected String getAppVersionInRequest(){
            return "paydroid" + "-" +  Utils.getResString(R.string.app_name) + "-" + MyApp.getAppVersion();
        }

    }

    // the base response for online request
    public static class Response{

        private Meta meta;

        public void setMeta(Meta meta) {
            this.meta = meta;
        }
        public Meta getMeta() {
            return meta;
        }


        public boolean needConfirm(String[] transStatusNotNeedConfirm){
            try {
                return isErrorCodeNeedConfirm() || isSubErrorCodeNeedConfirm()
                        || isTransStatusNeedConfirm(transStatusNotNeedConfirm);
            } catch (Exception e){
                Logger.error(e.getMessage());
            }
            return false;
        }

        private boolean isErrorCodeNeedConfirm(){
            return (isCode(ResponseConstants.CODE_02) && isErrorCodePrefix(ResponseConstants.ERROR_CODE_02)) ||
                    (isCode(ResponseConstants.CODE_09) && isErrorCodePrefix(ResponseConstants.ERROR_CODE_092));
        }

        private boolean isSubErrorCodeNeedConfirm(){
            return isCode(ResponseConstants.CODE_09) &&
                    isErrorCode(ResponseConstants.ERROR_CODE_09301) &&
                    (isSubErrorCode(ResponseConstants.PAYMENT_IN_PROGRESS) ||
                            isSubErrorCode(ResponseConstants.REQUEST_DUPLICATED));
        }

        private boolean isTransStatusNeedConfirm(String[] transStatusNotNeedConfirm){
            if (getBaseResult() == null || isIncluded(transStatusNotNeedConfirm, getBaseResult().getTransStatus())){
                //如果没有得到包含TransStatus的BaseResult，或者当前的transStatus包含在transStatusNotNeedConfirm中，那么不需要做confirm请求
                return false;
            }
            //当前的transStatus不在transStatusNotNeedConfirm中，并且 code 为 00，则需要做confirm请求
            return isCode(ResponseConstants.CODE_SUCCESS);
        }

        private boolean isTransStatus(String targetTransStatus){
            if (null != getBaseResult() && !TextUtils.isEmpty(getBaseResult().getTransStatus())){
                return TextUtils.equals(targetTransStatus, getBaseResult().getTransStatus());
            }
            return false;
        }

        public Result.Error getFailedInfo(String[] subErrorNotIncluded, String[] transStatusNotIncluded, boolean isE02AndE092Included){
            try {
                if (isFailedErrorCode(isE02AndE092Included)) {
                    if (TextUtils.isEmpty(getBaseData().getErrorInfo())) {
                        return new Result.Error(getBaseData().getErrorCode(),
                                ErrorCodeInfoUtils.getErrorCodeInfo(getBaseData().getErrorCode())).setSubErrorCode(getBaseData().getSubErrorCode());
                    }
                    return new Result.Error(getBaseData().getErrorCode(), getBaseData().getErrorInfo()).setSubErrorCode(getBaseData().getSubErrorCode());
                }
                if (isFailedSubErrorCode(subErrorNotIncluded)) {
                    return new Result.Error(getBaseData().getErrorCode(), SubErrCodeInfoUtils.getSubErrorCodeInfo(getBaseData().getSubErrorCode())).setSubErrorCode(getBaseData().getSubErrorCode());
                }
                if (isFailedTransStatus(transStatusNotIncluded)) {
                    return new Result.Error(getBaseResult().getTransStatus(), TransStatusErrorInfoUtils.getTransStatusErrorInfo(getBaseResult().getTransStatus())).setSubErrorCode(getBaseData().getSubErrorCode());
                }
            } catch (Exception e){
                Logger.error(e.getMessage());
            }
            return new Result.Error(ResponseConstants.UNKNOWN_ERROR_CODE).setSubErrorCode(getBaseData().getSubErrorCode());
        }

        /**
         * PosLink上位机获取上一笔交易做confirm时，生成对应的错误码给上位机
         * @param isPay
         * @return
         */
        public Result.Error getFailedInfoForTransactionStatus(boolean isPay){
            if (isCode(ResponseConstants.CODE_SUCCESS)){
                if (isPay){
                    if (isTransStatus(ResponseConstants.STATUS_REFUND_SUCCESS)){
                        return new Result.Error(
                                getBaseResult().getTransStatus(),
                                TransStatusErrorInfoUtils.getTransStatusErrorInfo(getBaseResult().getTransStatus()))
                                .setSubErrorCode(getBaseData().getSubErrorCode());
                    }
                } else {
                    if (isTransStatus(ResponseConstants.STATUS_PAY_SUCCESS)){
                        return new Result.Error(
                                getBaseResult().getTransStatus(),
                                TransStatusErrorInfoUtils.getTransStatusErrorInfo(getBaseResult().getTransStatus()))
                                .setSubErrorCode(getBaseData().getSubErrorCode());
                    }
                }
                if (isStatus(ResponseConstants.STATUS_REVERSE_SUCCESS)){
                    return new Result.Error(ResponseConstants.ERROR_CODE_REVERSE_SUCCESS);
                }
                if (isStatus(ResponseConstants.STATUS_PAY_FAILED)
                        || isStatus(ResponseConstants.STATUS_REFUND_FAILED)
                        || isStatus(ResponseConstants.STATUS_REVERSE_FAILED)){
                    return new Result.Error(
                            getBaseResult().getTransStatus(),
                            TransStatusErrorInfoUtils.getTransStatusErrorInfo(getBaseResult().getTransStatus()))
                            .setSubErrorCode(getBaseData().getSubErrorCode());
                }
                return new Result.Error(ResponseConstants.ERROR_CODE_REVERSE_WITHOUT_RESPONSE);//包括ResponseConstants.STATUS_PAYING等其他所有情况
            }
            if (isCode(ResponseConstants.CODE_09)){
                if (isErrorCode(ResponseConstants.ERROR_CODE_09301)) {
                    if (isSubErrorCode(ResponseConstants.PAYMENT_IN_PROGRESS)
                            || isSubErrorCode(ResponseConstants.REQUEST_DUPLICATED)) {
                        return new Result.Error(ResponseConstants.ERROR_CODE_REVERSE_WITHOUT_RESPONSE);
                    }
                    if (isSubErrorCode(ResponseConstants.TRADE_NOT_EXIST)
                            || isSubErrorCode(ResponseConstants.TRANSACTION_NOT_FOUND)) {
                        return new Result.Error(ResponseConstants.ERROR_CODE_REVERSE_SUCCESS);
                    }
                    Result.Error error = new Result.Error(
                            getBaseData().getErrorCode(),
                            SubErrCodeInfoUtils.getSubErrorCodeInfo(getBaseData().getSubErrorCode()))
                            .setSubErrorCode(getBaseData().getSubErrorCode());
                    error.setErrorCode(ResponseConstants.ERROR_CODE_L40);
                    return error;
                }
                if (isErrorCode(ResponseConstants.ERROR_CODE_09103)
                        || isErrorCode(ResponseConstants.ERROR_CODE_09117)
                        || isErrorCode(ResponseConstants.ERROR_CODE_09118)
                        || isErrorCode(ResponseConstants.ERROR_CODE_09119)
                        || isErrorCode(ResponseConstants.ERROR_CODE_09121)){
                    return new Result.Error(ResponseConstants.ERROR_CODE_REVERSE_SUCCESS);
                }
                if (isErrorCodePrefix(ResponseConstants.ERROR_CODE_091)
                    || isErrorCodePrefix(ResponseConstants.ERROR_CODE_099)){
                    //L40
                    Result.Error error = new Result.Error(
                            getBaseData().getErrorCode(),
                            ErrorCodeInfoUtils.getErrorCodeInfo(getBaseData().getErrorCode()))
                            .setSubErrorCode(getBaseData().getSubErrorCode());
                    error.setErrorCode(ResponseConstants.ERROR_CODE_L40);
                    return error;
                }
                if (isErrorCodePrefix(ResponseConstants.ERROR_CODE_092)){
                    return new Result.Error(ResponseConstants.ERROR_CODE_REVERSE_WITHOUT_RESPONSE);
                }
            }
            if (isCode(ResponseConstants.CODE_02) &&
                    isErrorCode(ResponseConstants.ERROR_CODE_02)){
                return new Result.Error(ResponseConstants.ERROR_CODE_REVERSE_WITHOUT_RESPONSE);
            }
            return new Result.Error(ResponseConstants.UNKNOWN_ERROR_CODE)
                    .setSubErrorCode(getBaseData().getSubErrorCode());
        }

        private boolean isFailedErrorCode(boolean isE02AndE092Included){
            if (isCode(ResponseConstants.CODE_09) &&
                    (isErrorCodePrefix(ResponseConstants.ERROR_CODE_091) ||
                            isErrorCodePrefix(ResponseConstants.ERROR_CODE_099))){
                return true;
            }
            if (isE02AndE092Included){
                if (isCode(ResponseConstants.CODE_02) &&
                        isErrorCodePrefix(ResponseConstants.ERROR_CODE_02)){
                    return true;
                }
                if (isCode(ResponseConstants.CODE_09) &&
                        isErrorCodePrefix(ResponseConstants.ERROR_CODE_092)){
                    return true;
                }
            }
            return false;
        }

        private boolean isFailedSubErrorCode(String[] subErrorCodeNotIncluded){
            if (isIncluded(subErrorCodeNotIncluded, getBaseData().getSubErrorCode())){
                return false;
            }
            return isCode(ResponseConstants.CODE_09) && isErrorCode(ResponseConstants.ERROR_CODE_09301);
        }

        private boolean isFailedTransStatus(String[] transStatusNotIncluded){
            if (isIncluded(transStatusNotIncluded, getBaseResult().getTransStatus())){
                return false;
            }
            return isCode(ResponseConstants.STATUS_PAY_SUCCESS);
        }

        /**
         * 如果target包含在arr中，则返回true
         * @param arr
         * @param target
         * @return
         */
        private boolean isIncluded(String[] arr, String target){
            if (null != arr && arr.length > 0){
                for (String str : arr){
                    if (TextUtils.equals(str, target)){
                        return true;
                    }
                }
            }
            return false;
        }

        public BaseData getBaseData(){
            return null;
        }

        public BaseResult getBaseResult(){
            return getBaseData().getBaseResult();
        }


        protected boolean isCode(String targetCode){
            return TextUtils.equals(targetCode, getMeta().getCode());
        }

        protected boolean isErrorCode(String targetErrorCode){
            return TextUtils.equals(targetErrorCode, getBaseData().getErrorCode());
        }

        protected boolean isStatus(String targetTransStatus){
            try {
                return TextUtils.equals(targetTransStatus, getBaseResult().getTransStatus());
            } catch (Exception e){
                Logger.error(e.getMessage());
            }
            return false;
        }

        protected boolean isSubErrorCode(String targetSubErrorCode){
            return TextUtils.equals(targetSubErrorCode, getBaseData().getSubErrorCode());
        }

        private boolean isErrorCodePrefix(String targetPrefix){
            return null != getBaseData().getErrorCode() && getBaseData().getErrorCode().startsWith(targetPrefix);
        }

    }

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
        return md5(stringBuilder.toString()).toUpperCase();
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
