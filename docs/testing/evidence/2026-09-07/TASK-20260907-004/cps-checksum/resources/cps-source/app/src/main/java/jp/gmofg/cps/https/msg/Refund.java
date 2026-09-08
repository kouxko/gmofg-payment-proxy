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
 * 20220325  	        JerryJia               	Create
 * ===========================================================================================
 */
package jp.gmofg.cps.https.msg;


import com.google.gson.Gson;

import jp.gmofg.cps.https.constants.ResponseConstants;
import jp.gmofg.cps.https.msg.products.Products;


public class Refund {

    // the request for refund
    public static class Request extends Message.Request{

        private int refundAmount;

        private String orderId;//optional, not used
        private String orderDetailId;//optional, not used
        private String qryToken;//optional
        private String currencyCode = "JPY";//optional
        private String refundReason;//optional, not used

        private String remark;//optional, not used
        private String appVersion = getAppVersionInRequest();//optional

        private Products products;

        public Request(String qryToken, int refundAmount, String token, String products){
            super();
            this.qryToken = qryToken;
            this.refundAmount = refundAmount;
            this.products = new Gson().fromJson(products, Products.class);
            generateSign(token);
        }
    }

    // the response for refund
    public static class Response extends Message.Response {

        private RefundData data;

        public void setData(RefundData data) {
            this.data = data;
        }
        public RefundData getBaseData() {
            return data;
        }

        public boolean isSuccessful(){
            return isCode(ResponseConstants.CODE_SUCCESS) && isStatus(ResponseConstants.STATUS_REFUND_SUCCESS);
        }
    }
}
