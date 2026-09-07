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

import jp.gmofg.cps.https.constants.ResponseConstants;
import jp.gmofg.cps.https.msg.products.Products;

public class Pay {
    // the request for pay
    public static class Request extends Message.Request{

        private String payType = "99";
        //optional
        private String currencyCode = "JPY";
        private int amount;
        private String receiptNo;//optional
        private String userCode;
        private String appVersion = getAppVersionInRequest();
        private String remark;//optional, not used
        private String extendInfo;//optional, not used

        private Products products;

        public Request(String tradeNo, int orderAmount, String authCode, String token, Products products){
            super();
            amount = orderAmount;
            receiptNo = tradeNo;
            userCode = authCode;
            this.products = products;
            generateSign(token);
        }
    }

    // the request for pay
    public static class Response extends Message.Response {

        private PayData data;

        public void setData(PayData data) {
            this.data = data;
        }

        @Override
        public PayData getBaseData() {
            return data;
        }

        //交易成功
        public boolean isSuccessful() {
            return isCode(ResponseConstants.CODE_SUCCESS) &&
                    isStatus(ResponseConstants.STATUS_PAY_SUCCESS);
        }

    }
}
