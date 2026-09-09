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

import jp.gmofg.cps.https.Utils;
import jp.gmofg.cps.https.constants.ResponseConstants;
import jp.gmofg.cps.utils.ErrorCodeInfoUtils;
import jp.gmofg.cps.utils.SubErrCodeInfoUtils;
import jp.gmofg.cps.utils.TransStatusErrorInfoUtils;


public class Reverse {
    // the request for revoke
    public static class Request extends Message.Request{

        private String orderId;//optional, not used
        private String appVersion = getAppVersionInRequest();
        private String qryToken;

        public Request(String qryToken, String token){
            super();
            this.qryToken = qryToken;
            generateSign(token);
        }
    }
    // the response for revoke
    public static class Response extends Message.Response{

        private ReverseData data;

        public void setData(ReverseData data) {
            this.data = data;
        }
        @Override
        public ReverseData getBaseData() {
            return data;
        }

        public boolean isSuccessful(){
            if (isCode(ResponseConstants.CODE_SUCCESS) &&
                    isStatus(ResponseConstants.STATUS_REVERSE_SUCCESS)){
                return true;
            }
            if (isCode(ResponseConstants.CODE_09) &&
                    (isErrorCode(ResponseConstants.ERROR_CODE_09103) ||
                            isErrorCode(ResponseConstants.ERROR_CODE_09117) ||
                            isErrorCode(ResponseConstants.ERROR_CODE_09118) ||
                            isErrorCode(ResponseConstants.ERROR_CODE_09119) ||
                            isErrorCode(ResponseConstants.ERROR_CODE_09121))){
                return true;
            }
            return isCode(ResponseConstants.CODE_09) &&
                    isErrorCode(ResponseConstants.ERROR_CODE_09301) &&
                    (isSubErrorCode(ResponseConstants.TRADE_NOT_EXIST) ||
                            isSubErrorCode(ResponseConstants.TRANSACTION_NOT_FOUND));
        }
    }

}
