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
 * 20220322  	        JerryJia               	Create
 * ===========================================================================================
 */
package jp.gmofg.cps.https.msg;

import jp.gmofg.cps.https.constants.ResponseConstants;
import jp.gmofg.cps.utils.ErrorCodeInfoUtils;
import jp.gmofg.cps.utils.SubErrCodeInfoUtils;
import jp.gmofg.cps.utils.TransStatusErrorInfoUtils;

public class Confirm {
    // the request for payQuery
    public static class Request extends Message.Request{

        private String orderId;//optional, not used
        private String orderDetailId;//optional, not used
        private String qryToken;//optional
        private String queryFlg = "1";//optional

        public Request(String qryToken, String token){
            super();
            this.qryToken = qryToken;
            generateSign(token);
        }
    }
    // the response for payQuery
    public static class Response extends Message.Response {

        private ConfirmData data;

        public void setData(ConfirmData data) {
            this.data = data;
        }
        public ConfirmData getBaseData() {
            return data;
        }

        public boolean isPaySuccessful(){
            return isCode(ResponseConstants.CODE_SUCCESS) &&
                    isStatus(ResponseConstants.STATUS_PAY_SUCCESS);
        }

        public boolean isRefundSuccessful(){
            return isCode(ResponseConstants.CODE_SUCCESS) &&
                    isStatus(ResponseConstants.STATUS_REFUND_SUCCESS);
        }

        public boolean isTradeNotExist(){
            if (!isCode(ResponseConstants.CODE_09)){
                return false;
            }
            return isErrorCode(ResponseConstants.ERROR_CODE_09103)
                    ||(isErrorCode(ResponseConstants.ERROR_CODE_09301) &&
                    (isSubErrorCode(ResponseConstants.TRADE_NOT_EXIST) || isSubErrorCode(ResponseConstants.TRANSACTION_NOT_FOUND)));
        }
    }

}
